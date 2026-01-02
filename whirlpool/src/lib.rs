
use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState, widgets};
use std::sync::Arc;
use std::f32::consts::PI;

// --- DSP COMPONENTS ---

// 1. Simple Reverb (Freeverb-ish)
struct Reverb {
    combs: Vec<(Vec<f32>, usize)>, // buffer, write_ptr
    allpasses: Vec<(Vec<f32>, usize)>,
    sample_rate: f32,
}

impl Reverb {
    fn new() -> Self {
        Self { combs: Vec::new(), allpasses: Vec::new(), sample_rate: 44100.0 }
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        // Tuning values from Freeverb
        let tuning_combs = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        let tuning_ap = [225, 341, 441, 561];

        self.combs = tuning_combs.iter().map(|&len| (vec![0.0; len], 0)).collect();
        self.allpasses = tuning_ap.iter().map(|&len| (vec![0.0; len], 0)).collect();
    }

    fn process(&mut self, input: f32, room_size: f32, damp: f32) -> f32 {
        let mut out = 0.0;
        let feedback = room_size * 0.28; // Scale 0-1 to reasonable feedback
        
        // Parallel Combs
        for (buffer, ptr) in self.combs.iter_mut() {
            let output = buffer[*ptr];
            buffer[*ptr] = input + (output * feedback * (1.0 - damp)); // Simple implementation
            *ptr = (*ptr + 1) % buffer.len();
            out += output;
        }

        // Series Allpasses
        for (buffer, ptr) in self.allpasses.iter_mut() {
            let buf_out = buffer[*ptr];
            let processed = out - buf_out;
            buffer[*ptr] = out + (buf_out * 0.5);
            *ptr = (*ptr + 1) % buffer.len();
            out = processed;
        }

        out * 0.015 // Gain compensation
    }
}

// 2. Simple Delay
struct StereoDelay {
    buffer: Vec<f32>,
    write_ptr: usize,
    sample_rate: f32,
}

impl StereoDelay {
    fn new() -> Self {
        Self { buffer: vec![0.0; 192000], write_ptr: 0, sample_rate: 44100.0 }
    }

    fn process(&mut self, left_in: f32, right_in: f32, time_ms: f32, feedback: f32) -> (f32, f32) {
        let delay_samples = (time_ms / 1000.0 * self.sample_rate).max(1.0);
        
        // Read ptr
        let read_idx_f = self.write_ptr as f32 - delay_samples;
        let read_idx = if read_idx_f < 0.0 { read_idx_f + self.buffer.len() as f32 } else { read_idx_f };
        let idx = read_idx as usize; // No lerp for simplicity in this turn, add if grainy

        // Simple mono buffer for stereo delay (ping pong or just dual mono would be better, but sharing buffer for texture)
        // Let's do simple mono delay logic applied to stereo for "Whirlpool" wash
        let delayed = self.buffer[idx];
        
        // Write
        let input_mix = (left_in + right_in) * 0.5;
        self.buffer[self.write_ptr] = input_mix + (delayed * feedback);
        self.write_ptr = (self.write_ptr + 1) % self.buffer.len();

        (delayed, delayed) // Dual mono return
    }
}

// 3. Multi-Band Slam (OTT approximation)
struct OttBand {
    ceiling: f32,
}

impl OttBand {
    fn process(&mut self, input: f32, depth: f32, drive: f32) -> f32 {
        // Hard upward/downward compression sim
        let driven = input * (1.0 + drive * 10.0); // Boost input
        let compressed = driven.tanh(); // Soft clip / Limit
        
        // "Upward" check (boost quiet sounds - simplified by just high gain + limit)
        // Depth blends between dry and smashed
        input * (1.0 - depth) + compressed * depth
    }
}

// --- PLUGIN ---

struct Whirlpool {
    params: Arc<WhirlpoolParams>,
    reverb: Reverb,
    delay: StereoDelay,
    // Simple localized state for OTT
}

#[derive(Params)]
struct WhirlpoolParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    // Reverb
    #[id = "rev_mix"] pub rev_mix: FloatParam,
    #[id = "rev_size"] pub rev_size: FloatParam,

    // Delay
    #[id = "del_time"] pub del_time: FloatParam,
    #[id = "del_feed"] pub del_feed: FloatParam,
    #[id = "del_mix"] pub del_mix: FloatParam,

    // OTT
    #[id = "ott_depth"] pub ott_depth: FloatParam,
    #[id = "ott_drive"] pub ott_drive: FloatParam,

    #[id = "output_gain"] pub output_gain: FloatParam,
}

impl Default for Whirlpool {
    fn default() -> Self {
        Self {
            params: Arc::new(WhirlpoolParams::default()),
            reverb: Reverb::new(),
            delay: StereoDelay::new(),
        }
    }
}

impl Default for WhirlpoolParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(400, 500),

            rev_mix: FloatParam::new("Rev Mix", 0.3, FloatRange::Linear { min: 0.0, max: 1.0 }),
            rev_size: FloatParam::new("Rev Size", 0.8, FloatRange::Linear { min: 0.1, max: 0.99 }),

            del_time: FloatParam::new("Dly Time", 250.0, FloatRange::Linear { min: 10.0, max: 2000.0 }),
            del_feed: FloatParam::new("Dly Feed", 0.4, FloatRange::Linear { min: 0.0, max: 0.95 }),
            del_mix: FloatParam::new("Dly Mix", 0.3, FloatRange::Linear { min: 0.0, max: 1.0 }),

            ott_depth: FloatParam::new("OTT Depth", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 }),
            ott_drive: FloatParam::new("OTT Drive", 0.2, FloatRange::Linear { min: 0.0, max: 1.0 }),

            output_gain: FloatParam::new("Out Gain", 1.0, FloatRange::Linear { min: 0.0, max: 2.0 }),
        }
    }
}

impl Plugin for Whirlpool {
    const NAME: &'static str = "Whirlpool";
    const VENDOR: &'static str = "Antigravity";
    const URL: &'static str = "https://example.com";
    const EMAIL: &'static str = "info@example.com";
    const VERSION: &'static str = "1.0.0";

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

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.reverb.init(buffer_config.sample_rate);
        self.delay.sample_rate = buffer_config.sample_rate;
        true
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |_, _| {},
            move |ctx: &egui::Context, setter: &ParamSetter, _state: &mut ()| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("WHIRLPOOL");
                    ui.separator();
                    
                    // Style
                    ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 10.0);

                    ui.push_id("reverb_sect", |ui| {
                        ui.label(egui::RichText::new("REVERB (Wash)").strong());
                        ui.add(widgets::ParamSlider::for_param(&params.rev_mix, setter));
                        ui.add(widgets::ParamSlider::for_param(&params.rev_size, setter));
                    });
                    ui.separator();

                    ui.push_id("delay_sect", |ui| {
                        ui.label(egui::RichText::new("DELAY (Flow)").strong());
                        ui.add(widgets::ParamSlider::for_param(&params.del_time, setter));
                        ui.add(widgets::ParamSlider::for_param(&params.del_feed, setter));
                        ui.add(widgets::ParamSlider::for_param(&params.del_mix, setter));
                    });
                    ui.separator();

                    ui.push_id("ott_sect", |ui| {
                        ui.label(egui::RichText::new("OTT (Crush)").strong());
                        ui.add(widgets::ParamSlider::for_param(&params.ott_depth, setter));
                        ui.add(widgets::ParamSlider::for_param(&params.ott_drive, setter));
                    });
                    ui.separator();
                    
                    ui.add(widgets::ParamSlider::for_param(&params.output_gain, setter));
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
        for channel_samples in buffer.iter_samples() {
            let rev_mix = self.params.rev_mix.value();
            let rev_size = self.params.rev_size.value();
            
            let del_time = self.params.del_time.value();
            let del_feed = self.params.del_feed.value();
            let del_mix = self.params.del_mix.value();
            
            let ott_depth = self.params.ott_depth.value();
            let ott_drive = self.params.ott_drive.value();
            
            let out_gain = self.params.output_gain.value();

            // Collect samples to mutable references so we can read and write
            let mut samples: Vec<&mut f32> = channel_samples.into_iter().collect();
            
            if samples.len() < 2 {
                continue; 
            }

            // READ
            let mut buf_l = *samples[0];
            let mut buf_r = *samples[1];
            
            // --- REVERB ---
            let mono_sum = (buf_l + buf_r) * 0.5;
            let rev_sig = self.reverb.process(mono_sum, rev_size, 0.5);
            
            // Blend Reverb
            buf_l = buf_l * (1.0 - rev_mix) + rev_sig * rev_mix;
            buf_r = buf_r * (1.0 - rev_mix) + rev_sig * rev_mix;

            // --- DELAY ---
            let (d_l, d_r) = self.delay.process(buf_l, buf_r, del_time, del_feed);
            buf_l = buf_l * (1.0 - del_mix) + d_l * del_mix;
            buf_r = buf_r * (1.0 - del_mix) + d_r * del_mix;

            // --- OTT (Simplified Multiband Slam) ---
            let smash = |x: f32| -> f32 {
                let driven = x * (1.0 + ott_drive * 10.0);
                driven.tanh()
            };
            
            let smashed_l = smash(buf_l);
            let smashed_r = smash(buf_r);
            
            buf_l = buf_l * (1.0 - ott_depth) + smashed_l * ott_depth;
            buf_r = buf_r * (1.0 - ott_depth) + smashed_r * ott_depth;

            // WRITE
            *samples[0] = buf_l * out_gain;
            *samples[1] = buf_r * out_gain;
        }

        ProcessStatus::Normal
    }
}

impl Vst3Plugin for Whirlpool {
    const VST3_CLASS_ID: [u8; 16] = *b"Whirlpool_______";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Reverb, Vst3SubCategory::Delay];
}

impl ClapPlugin for Whirlpool {
    const CLAP_ID: &'static str = "com.antigravity.whirlpool";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Whirlpool Reverb-Delay-Slam");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Stereo];
}

nih_export_clap!(Whirlpool);
nih_export_vst3!(Whirlpool);
