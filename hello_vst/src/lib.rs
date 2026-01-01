
use nih_plug::prelude::*;
use std::sync::Arc;
use rand::Rng;

struct HelloVst {
    params: Arc<HelloVstParams>,
    delay_buffer: Vec<f32>,
    write_ptr: usize,
    
    // Granular state
    current_delay_samples: f32,
    target_delay_samples: f32,
    samples_until_next_grain: usize,
}

#[derive(Params)]
struct HelloVstParams {
    #[id = "feedback"]
    pub feedback: FloatParam,

    #[id = "mix"]
    pub mix: FloatParam,
    
    #[id = "delay_time"]
    pub delay_time: FloatParam, 

    #[id = "jitter"]
    pub jitter: FloatParam, 

    #[id = "grain_size"]
    pub grain_size: FloatParam, 
}

impl Default for HelloVst {
    fn default() -> Self {
        Self {
            params: Arc::new(HelloVstParams::default()),
            delay_buffer: vec![0.0; 192000], // ~4 sec buffer
            write_ptr: 0,
            current_delay_samples: 0.0,
            target_delay_samples: 0.0,
            samples_until_next_grain: 0,
        }
    }
}

impl Default for HelloVstParams {
    fn default() -> Self {
        Self {
            feedback: FloatParam::new(
                "Feedback",
                0.5,
                FloatRange::Linear { min: 0.0, max: 0.95 },
            ).with_unit(" %"),
            
            mix: FloatParam::new(
                "Mix",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ).with_unit(" %"),

            delay_time: FloatParam::new(
                "Delay Time",
                200.0,
                FloatRange::Skewed { min: 10.0, max: 2000.0, factor: 0.5 },
            ).with_unit(" ms"),

            jitter: FloatParam::new(
                "Jitter",
                50.0,
                FloatRange::Linear { min: 0.0, max: 500.0 },
            ).with_unit(" ms"),

            grain_size: FloatParam::new(
                "Grain Rate",
                50.0,
                 FloatRange::Skewed { min: 10.0, max: 500.0, factor: 0.5 },
            ).with_unit(" ms"),
        }
    }
}

impl Plugin for HelloVst {
    const NAME: &'static str = "Granular Delay";
    const VENDOR: &'static str = "Antigravity";
    const URL: &'static str = "https://example.com";
    const EMAIL: &'static str = "info@example.com";
    const VERSION: &'static str = "0.1.0";

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

    // Default editor (Generic)
    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        None 
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let sample_rate = context.transport().sample_rate;
        let mut rng = rand::thread_rng();

        for channel_samples in buffer.iter_samples() {
            let delay_time_ms = self.params.delay_time.value();
            let jitter_ms = self.params.jitter.value();
            let grain_period_ms = self.params.grain_size.value();
            let feedback_amt = self.params.feedback.value();
            let mix_amt = self.params.mix.value();

            // Granular update logic
            if self.samples_until_next_grain == 0 {
                let jitter_sample_range = (jitter_ms / 1000.0 * sample_rate) as f32;
                let base_delay_samples = (delay_time_ms / 1000.0 * sample_rate) as f32;
                
                let random_offset = rng.gen_range(-jitter_sample_range..=jitter_sample_range);
                self.target_delay_samples = (base_delay_samples + random_offset).max(0.0);
                
                let period_samples = (grain_period_ms / 1000.0 * sample_rate) as usize;
                self.samples_until_next_grain = period_samples.max(1);
            }
            self.samples_until_next_grain -= 1;

            // Smooth delay modulation (simple lerp)
            self.current_delay_samples += (self.target_delay_samples - self.current_delay_samples) * 0.01;

            // Audio processing
            let mut left_in = 0.0;
            let mut right_in = 0.0;
            
            // Calculate read position
            let delay_sub = self.current_delay_samples;
            let read_idx_f32 = self.write_ptr as f32 - delay_sub;
            let read_idx = if read_idx_f32 < 0.0 {
                read_idx_f32 + self.delay_buffer.len() as f32
            } else {
                read_idx_f32
            };
            
            // Linear interpolation read
            let idx_floor = read_idx.floor() as usize;
            let idx_ceil = (idx_floor + 1) % self.delay_buffer.len();
            let alpha = read_idx - read_idx.floor();
            
            let delayed_sample = self.delay_buffer[idx_floor] * (1.0 - alpha) + self.delay_buffer[idx_ceil] * alpha;

            // Process channels (Read & Write in one pass)
            for (i, sample) in channel_samples.into_iter().enumerate() {
                let input = *sample;
                if i == 0 { 
                    left_in = input; 
                    *sample = input * (1.0 - mix_amt) + delayed_sample * mix_amt;
                }
                else if i == 1 { 
                    right_in = input; 
                    *sample = input * (1.0 - mix_amt) + delayed_sample * mix_amt;
                }
            }

            // Write to delay buffer
            let mono_in = (left_in + right_in) * 0.5;
            self.delay_buffer[self.write_ptr] = mono_in + (delayed_sample * feedback_amt);
            self.write_ptr = (self.write_ptr + 1) % self.delay_buffer.len();
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for HelloVst {
    const CLAP_ID: &'static str = "com.antigravity.granular-delay";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Granular Delay VST");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Utility];
}

impl Vst3Plugin for HelloVst {
    const VST3_CLASS_ID: [u8; 16] = *b"GranularDelay___";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx];
}

nih_export_clap!(HelloVst);
nih_export_vst3!(HelloVst);
