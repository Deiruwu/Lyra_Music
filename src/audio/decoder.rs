use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecRegistry, Decoder as SymphCodecDecoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use rubato::{Resampler, SincFixedOut, SincInterpolationType, SincInterpolationParameters, WindowFunction};
use std::fs::File;
use std::path::Path;
use std::time::Duration;
use symphonia_adapter_libopus::OpusDecoder;
use crate::audio::errors::decode_error::DecodeError;
use crate::model::audio_tech::{AudioProperties, PlayableTrack};
use crate::model::Track;

// --- DTO & ERRORES ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMode {
    Stereo,
    MonoMix,
}

pub const TARGET_SAMPLE_RATE: u32 = 48000;
pub const TARGET_CHANNELS: usize = 2;

pub trait AudioDecoder: Send {
    fn decode_next(&mut self) -> Result<Option<Vec<f32>>, DecodeError>;
    fn seek(&mut self, target: Duration) -> Result<(), DecodeError>;
    fn properties(&self) -> AudioProperties;
}

// ─── FUNCIÓN AUXILIAR: REGISTRO DE CODECS ──────────────────────────────────
// Centraliza la inicialización para asegurar que Opus esté disponible
// tanto para decodificar como para extraer metadata (probe).
fn build_codec_registry() -> CodecRegistry {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_all::<OpusDecoder>();
    registry
}
// ───────────────────────────────────────────────────────────────────────────

pub struct SymphoniaDecoder {
    format_reader: Box<dyn FormatReader>,
    decoder: Box<dyn SymphCodecDecoder>,
    track_id: u32,
    properties: AudioProperties,
    sample_buf: Option<SampleBuffer<f32>>,
    resampler: Option<SincFixedOut<f32>>,
    resample_staging: Vec<Vec<f32>>,
    mode: ChannelMode,
}

impl SymphoniaDecoder {
    pub fn open<P: AsRef<Path>>(path: P, mode: ChannelMode) -> Result<Self, DecodeError> {
        let path_ref = path.as_ref();
        let file = File::open(path_ref)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path_ref.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| DecodeError::Format(e.to_string()))?;

        let format_reader = probed.format;

        let track = format_reader
            .tracks()
            .iter()
            .find(|t| {
                t.codec_params.codec != CODEC_TYPE_NULL
                    && t.codec_params.sample_rate.is_some()
            })
            .ok_or(DecodeError::NoAudioStream)?;

        let track_id = track.id;
        let params = &track.codec_params;
        let source_sample_rate = params.sample_rate.unwrap_or(TARGET_SAMPLE_RATE);

        // Usamos nuestro registro inyectado
        let codec_registry = build_codec_registry();

        let decoder = codec_registry
            .make(params, &DecoderOptions::default())
            .map_err(|e| DecodeError::Codec(e.to_string()))?;

        let properties = AudioProperties {
            sample_rate: source_sample_rate,
            channels: params.channels.map(|c| c.count() as u8).unwrap_or(TARGET_CHANNELS as u8),
            bit_depth: params.bits_per_sample.map(|b| b as u8),
            codec: codec_registry
                .get_codec(params.codec)
                .map(|d| d.short_name.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            duration_secs: params.n_frames
                .zip(params.sample_rate)
                .map(|(frames, rate)| frames / rate as u64),
        };

        let mut resampler = None;
        let resample_staging = vec![Vec::<f32>::new(); TARGET_CHANNELS];

        if source_sample_rate != TARGET_SAMPLE_RATE {
            resampler = Some(make_resampler(source_sample_rate, TARGET_SAMPLE_RATE)?);
        }

        {
            let channels = properties.channels;
            let codec    = &properties.codec;
            let ch_label = match channels { 1 => "Mono", 2 => "Stereo", 6 => "5.1", 8 => "7.1", _ => "?" };
            let rate_col = if source_sample_rate == TARGET_SAMPLE_RATE {
                format!("{source_sample_rate} Hz (nativo)")
            } else {
                format!("{source_sample_rate} Hz → {TARGET_SAMPLE_RATE} Hz  ⚠ puede ajustarse en primer frame (HE-AAC/SBR)")
            };
            eprintln!("[DECODER]  codec={codec}  canales={channels} ({ch_label})  rate={rate_col}");
        }

        Ok(Self {
            format_reader,
            decoder,
            track_id,
            properties,
            sample_buf: None,
            resampler,
            resample_staging,
            mode,
        })
    }
}

fn make_resampler(from_rate: u32, to_rate: u32) -> Result<SincFixedOut<f32>, DecodeError> {
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    SincFixedOut::<f32>::new(
        to_rate as f64 / from_rate as f64,
        2.0,
        params,
        1024,
        TARGET_CHANNELS,
    ).map_err(|e| DecodeError::Resample(e.to_string()))
}

impl AudioDecoder for SymphoniaDecoder {
    fn decode_next(&mut self) -> Result<Option<Vec<f32>>, DecodeError> {
        loop {
            let packet = match self.format_reader.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                    {
                        return Ok(None);
                    }
                Err(e) => return Err(DecodeError::Format(e.to_string())),
            };

            if packet.track_id() != self.track_id {
                continue;
            }

            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let actual_rate = decoded.spec().rate;
                    if actual_rate != self.properties.sample_rate {
                        let header_rate = self.properties.sample_rate;
                        self.properties.sample_rate = actual_rate;

                        for ch in &mut self.resample_staging {
                            ch.clear();
                        }

                        if actual_rate != TARGET_SAMPLE_RATE {
                            self.resampler = Some(make_resampler(actual_rate, TARGET_SAMPLE_RATE)?);
                            eprintln!(
                                "[DECODER]  ⚠ HE-AAC/SBR  header={header_rate} Hz (incorrecto)  real={actual_rate} Hz  rate={actual_rate} Hz → {TARGET_SAMPLE_RATE} Hz"
                            );
                        } else {
                            self.resampler = None;
                            eprintln!(
                                "[DECODER]  ⚠ HE-AAC/SBR  header={header_rate} Hz (incorrecto)  real={actual_rate} Hz  rate={actual_rate} Hz (nativo, sin resampler)"
                            );
                        }
                    }

                    if self.sample_buf.is_none() {
                        self.sample_buf = Some(SampleBuffer::<f32>::new(
                            decoded.capacity() as u64,
                            *decoded.spec(),
                        ));
                    }
                    let buf = self.sample_buf.as_mut().unwrap();
                    buf.copy_interleaved_ref(decoded);
                    let raw_samples = buf.samples();
                    let source_channels = self.properties.channels as usize;

                    if self.resampler.is_none() {
                        let mut out = Vec::with_capacity(raw_samples.len());
                        for chunk in raw_samples.chunks(source_channels) {
                            let l = chunk[0];
                            let r = if source_channels > 1 { chunk[1] } else { chunk[0] };
                            match self.mode {
                                ChannelMode::Stereo  => { out.push(l); out.push(r); }
                                ChannelMode::MonoMix => { let m = (l + r) * 0.5; out.push(m); out.push(m); }
                            }
                        }
                        return Ok(Some(out));
                    }

                    for frame in raw_samples.chunks(source_channels) {
                        self.resample_staging[0].push(frame[0]);
                        self.resample_staging[1].push(
                            if source_channels > 1 { frame[1] } else { frame[0] }
                        );
                    }

                    let mut out = Vec::new();
                    loop {
                        let needed = self.resampler.as_mut().unwrap().input_frames_next();
                        if self.resample_staging[0].len() < needed {
                            break;
                        }

                        let input: Vec<Vec<f32>> = self.resample_staging
                            .iter_mut()
                            .map(|ch| ch.drain(..needed).collect())
                            .collect();

                        let resampled = self.resampler
                            .as_mut()
                            .unwrap()
                            .process(&input, None)
                            .map_err(|e| DecodeError::Resample(e.to_string()))?;

                        for i in 0..resampled[0].len() {
                            let l = resampled[0][i];
                            let r = resampled[1][i];
                            match self.mode {
                                ChannelMode::Stereo  => { out.push(l); out.push(r); }
                                ChannelMode::MonoMix => { let m = (l + r) * 0.5; out.push(m); out.push(m); }
                            }
                        }
                    }

                    if out.is_empty() {
                        continue;
                    }

                    return Ok(Some(out));
                }
                Err(symphonia::core::errors::Error::DecodeError(e)) => {
                    eprintln!("[DECODER WARN] Frame corrupto saltado: {}", e);
                    continue;
                }
                Err(e) => return Err(DecodeError::Codec(e.to_string())),
            }
        }
    }

    fn seek(&mut self, target: Duration) -> Result<(), DecodeError> {
        let symphonia_time = symphonia::core::units::Time::from(target.as_secs_f64());

        self.format_reader.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: symphonia_time,
                track_id: Some(self.track_id),
            },
        ).map_err(|e| DecodeError::Format(format!("Seek falló: {}", e)))?;

        self.decoder.reset();

        for ch in &mut self.resample_staging {
            ch.clear();
        }
        if let Some(r) = &mut self.resampler {
            r.reset();
        }

        Ok(())
    }

    fn properties(&self) -> AudioProperties {
        self.properties.clone()
    }
}

pub fn probe_file<P: AsRef<Path>>(path: P, track: Track) -> Result<PlayableTrack, DecodeError> {
    let path_ref = path.as_ref();
    let file = File::open(path_ref)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path_ref.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| DecodeError::Format(e.to_string()))?;

    let format = probed.format;

    let audio_track = format
        .tracks()
        .iter()
        .find(|t| {
            t.codec_params.codec != CODEC_TYPE_NULL
                && t.codec_params.sample_rate.is_some()
        })
        .ok_or(DecodeError::NoAudioStream)?;

    let params = &audio_track.codec_params;

    // Usamos nuestro registro inyectado también aquí
    let codec_registry = build_codec_registry();

    let audio_props = AudioProperties {
        sample_rate: params.sample_rate.unwrap_or(48000),
        channels: params.channels.map(|c| c.count() as u8).unwrap_or(2),
        bit_depth: params.bits_per_sample.map(|b| b as u8),
        codec: codec_registry
            .get_codec(params.codec)
            .map(|d| d.short_name.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        duration_secs: params.n_frames
            .zip(params.sample_rate)
            .map(|(frames, rate)| frames / rate as u64),
    };

    Ok(PlayableTrack {
        track,
        audio: audio_props,
    })
}