use symphonia::core::codecs::audio::{AudioDecoder as SymphAudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use rubato::{Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use std::fs::File;
use std::path::Path;
use std::sync::LazyLock;
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
}

// ─── FUNCIÓN AUXILIAR: REGISTRO DE CODECS ──────────────────────────────────
// Centraliza la inicialización para asegurar que Opus esté disponible
// tanto para decodificar como para extraer metadata (probe).
/// El registro es de solo lectura una vez construido, así que se comparte.
/// Antes se reconstruía entero en cada `open()` y en cada `probe_file()`, o
/// sea dos veces por cambio de canción.
static CODEC_REGISTRY: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_audio_decoder::<OpusDecoder>();
    registry
});
// ───────────────────────────────────────────────────────────────────────────

pub struct SymphoniaDecoder {
    format_reader: Box<dyn FormatReader>,
    decoder: Box<dyn SymphAudioDecoder>,
    track_id: u32,
    properties: AudioProperties,
    raw_buf: Vec<f32>,
    resampler: Option<Async<f32>>,
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

        let format_reader = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| DecodeError::Format(e.to_string()))?;

        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.as_ref().is_some_and(|cp| cp.audio().is_some()))
            .ok_or(DecodeError::NoAudioStream)?;

        let track_id = track.id;
        let audio_params = track
            .codec_params
            .as_ref()
            .and_then(|cp| cp.audio())
            .ok_or(DecodeError::NoAudioStream)?;
        let source_sample_rate = audio_params.sample_rate.unwrap_or(TARGET_SAMPLE_RATE);

        // Usamos nuestro registro inyectado
        let codec_registry = &*CODEC_REGISTRY;

        let decoder = codec_registry
            .make_audio_decoder(audio_params, &AudioDecoderOptions::default())
            .map_err(|e| DecodeError::Codec(e.to_string()))?;

        let properties = AudioProperties {
            sample_rate: source_sample_rate,
            channels: audio_params.channels.as_ref().map(|c| c.count() as u8).unwrap_or(TARGET_CHANNELS as u8),
            bit_depth: audio_params.bits_per_sample.map(|b| b as u8),
            codec: codec_registry
                .get_audio_decoder(audio_params.codec)
                .map(|d| d.codec.info.short_name.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            duration_secs: track.num_frames
                .zip(audio_params.sample_rate)
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
            raw_buf: Vec::new(),
            resampler,
            resample_staging,
            mode,
        })
    }
}

fn make_resampler(from_rate: u32, to_rate: u32) -> Result<Async<f32>, DecodeError> {
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: Some(0.95),
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    Async::<f32>::new_sinc(
        to_rate as f64 / from_rate as f64,
        2.0,
        &params,
        1024,
        TARGET_CHANNELS,
        FixedAsync::Output,
    ).map_err(|e| DecodeError::Resample(e.to_string()))
}

impl AudioDecoder for SymphoniaDecoder {
    fn decode_next(&mut self) -> Result<Option<Vec<f32>>, DecodeError> {
        loop {
            let packet = match self.format_reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(e) => return Err(DecodeError::Format(e.to_string())),
            };

            if packet.track_id != self.track_id {
                continue;
            }

            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let actual_rate = decoded.spec().rate();
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

                    self.raw_buf.clear();
                    decoded.copy_to_vec_interleaved(&mut self.raw_buf);
                    let raw_samples = &self.raw_buf[..];
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

                        let adapter = SequentialSliceOfVecs::new(&input, TARGET_CHANNELS, needed)
                            .map_err(|e| DecodeError::Resample(e.to_string()))?;

                        let resampled = self.resampler
                            .as_mut()
                            .unwrap()
                            .process(&adapter, None)
                            .map_err(|e| DecodeError::Resample(e.to_string()))?;

                        let interleaved = resampled.take_data();

                        match self.mode {
                            ChannelMode::Stereo => out.extend(interleaved),
                            ChannelMode::MonoMix => {
                                for frame in interleaved.chunks(2) {
                                    let m = (frame[0] + frame[1]) * 0.5;
                                    out.push(m);
                                    out.push(m);
                                }
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
        let symphonia_time = symphonia::core::units::Time::try_from_secs_f64(target.as_secs_f64())
            .ok_or_else(|| DecodeError::Format("Duración de seek inválida".to_string()))?;

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

}

pub fn probe_file<P: AsRef<Path>>(path: P, track: Track) -> Result<PlayableTrack, DecodeError> {
    let path_ref = path.as_ref();
    let file = File::open(path_ref)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path_ref.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| DecodeError::Format(e.to_string()))?;

    let audio_track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.as_ref().is_some_and(|cp| cp.audio().is_some()))
        .ok_or(DecodeError::NoAudioStream)?;

    let audio_params = audio_track
        .codec_params
        .as_ref()
        .and_then(|cp| cp.audio())
        .ok_or(DecodeError::NoAudioStream)?;

    // Usamos nuestro registro inyectado también aquí
    let codec_registry = &*CODEC_REGISTRY;

    let audio_props = AudioProperties {
        sample_rate: audio_params.sample_rate.unwrap_or(48000),
        channels: audio_params.channels.as_ref().map(|c| c.count() as u8).unwrap_or(2),
        bit_depth: audio_params.bits_per_sample.map(|b| b as u8),
        codec: codec_registry
            .get_audio_decoder(audio_params.codec)
            .map(|d| d.codec.info.short_name.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        duration_secs: audio_track.num_frames
            .zip(audio_params.sample_rate)
            .map(|(frames, rate)| frames / rate as u64),
    };

    Ok(PlayableTrack {
        track,
        audio: audio_props,
    })
}
