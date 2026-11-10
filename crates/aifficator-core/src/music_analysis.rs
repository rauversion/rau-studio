//! Rau's offline, constant-tempo musical analysis. No upstream analyzer code is
//! used. FFT, positive spectral change, periodic folding and musical scale
//! templates are implemented here independently. Results are proposals, not
//! measured probabilities or a substitute for listening to a corrected grid.

use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use std::process::{Child, Command, Stdio};

const RATE: usize = 11_025;
const HOP: usize = 128;
const ONSET_SIZE: usize = 1024;
const KEY_SIZE: usize = 4096;
const MAX_SECONDS: usize = 7200;
pub const VERSION: &str = "rau-constant-4-4-v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MusicAnalysis {
    pub version: String,
    pub duration_seconds: f64,
    pub bpm: Option<f64>,
    pub tempo_confidence: f64,
    pub anchor_seconds: Option<f64>,
    pub beat_in_bar: u16,
    pub downbeat_confidence: f64,
    pub key: Option<String>,
    pub key_confidence: f64,
    pub waveform_hz: u32,
    pub waveform: Vec<u8>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GridCorrection {
    pub bpm: f64,
    /// Time of any identified beat. Earlier beats are extrapolated to file start.
    pub anchor_seconds: f64,
    pub beat_in_bar: u16,
}

impl GridCorrection {
    pub fn validate(&self, duration: f64) -> Result<(), String> {
        if !duration.is_finite()
            || duration <= 0.0
            || !self.bpm.is_finite()
            || !(30.0..=300.0).contains(&self.bpm)
            || !self.anchor_seconds.is_finite()
            || self.anchor_seconds < 0.0
            || self.anchor_seconds >= duration
            || !(1..=4).contains(&self.beat_in_bar)
        {
            return Err("La grilla requiere BPM entre 30 y 300, un tiempo dentro de la pista y un beat entre 1 y 4.".into());
        }
        Ok(())
    }

    pub fn first_anchor(&self) -> (f64, u16) {
        let period = 60.0 / self.bpm;
        let preceding = (self.anchor_seconds / period).floor() as u64;
        let seconds = (self.anchor_seconds - preceding as f64 * period).max(0.0);
        let beat = (i64::from(self.beat_in_bar) - 1 - (preceding % 4) as i64).rem_euclid(4) + 1;
        (seconds, beat as u16)
    }
}

/// Restrict manual keys to a small, unambiguous classic notation. Empty means
/// keep existing metadata. Enharmonic spelling is preserved when supplied.
pub fn validate_key(key: Option<&str>) -> Result<(), String> {
    if let Some(key) = key {
        let root = key.strip_suffix('m').unwrap_or(key);
        if !matches!(
            root,
            "C" | "C#"
                | "Db"
                | "D"
                | "D#"
                | "Eb"
                | "E"
                | "F"
                | "F#"
                | "Gb"
                | "G"
                | "G#"
                | "Ab"
                | "A"
                | "A#"
                | "Bb"
                | "B"
        ) {
            return Err("Elige una tonalidad mayor o menor válida.".into());
        }
    }
    Ok(())
}

struct Decoder(Child);
impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Decode a bounded stream into local DSP. The source is opened read-only by
/// FFmpeg; no tags, audio files, installed libraries or devices are written.
pub fn analyze_file(
    path: &Path,
    ffmpeg: &Path,
    min_bpm: f64,
    max_bpm: f64,
) -> Result<MusicAnalysis, String> {
    if !min_bpm.is_finite()
        || !max_bpm.is_finite()
        || min_bpm < 40.0
        || max_bpm > 300.0
        || max_bpm - min_bpm < 10.0
    {
        return Err(
            "El rango de análisis debe estar entre 40 y 300 BPM y abarcar al menos 10 BPM.".into(),
        );
    }
    let error_file = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut error_reader = error_file.try_clone().map_err(|e| e.to_string())?;
    let child = Command::new(ffmpeg)
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-xerror",
            "-i",
        ])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-t",
            "7201",
            "-ac",
            "1",
            "-ar",
            "11025",
            "-c:a",
            "pcm_f32le",
            "-f",
            "f32le",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(error_file))
        .spawn()
        .map_err(|e| format!("No se pudo iniciar el análisis de audio: {e}"))?;
    let mut decoder = Decoder(child);
    let mut input = BufReader::new(
        decoder
            .0
            .stdout
            .take()
            .ok_or("No se pudo leer el audio decodificado.")?,
    );
    let mut analyzer = Analyzer::new();
    let mut bytes = [0; 4];
    loop {
        // read_exact alone would silently accept a partial final f32 as EOF.
        match input.read(&mut bytes[..1]).map_err(|e| e.to_string())? {
            0 => break,
            _ => input
                .read_exact(&mut bytes[1..])
                .map_err(|e| format!("Audio decodificado incompleto: {e}"))?,
        }
        let sample = f32::from_le_bytes(bytes);
        if !sample.is_finite() || analyzer.samples >= RATE * MAX_SECONDS {
            return Err(
                "El análisis admite hasta dos horas de audio y requiere muestras finitas.".into(),
            );
        }
        analyzer.push(sample, true);
    }
    let status = decoder.0.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        error_reader.rewind().map_err(|e| e.to_string())?;
        let mut message = String::new();
        error_reader
            .take(4096)
            .read_to_string(&mut message)
            .map_err(|e| e.to_string())?;
        return Err(format!(
            "No se pudo decodificar la pista: {}",
            message.trim()
        ));
    }
    if analyzer.samples < RATE * 2 {
        return Err("La pista necesita al menos dos segundos de audio para analizarse.".into());
    }
    Ok(analyzer.finish(min_bpm, max_bpm))
}

/// Radix-2 DFT with precomputed roots and permutation; no DSP dependency or
/// vendor implementation is embedded. Both fixed transform sizes are powers
/// of two and the input is real, Hann-windowed audio.
struct Spectrum {
    size: usize,
    permutation: Vec<usize>,
    roots: Vec<(f64, f64)>,
    window: Vec<f64>,
    real: Vec<f64>,
    imag: Vec<f64>,
}
impl Spectrum {
    fn new(size: usize) -> Self {
        let bits = size.trailing_zeros();
        Self {
            size,
            permutation: (0..size)
                .map(|i| i.reverse_bits() >> (usize::BITS - bits))
                .collect(),
            roots: (0..size / 2)
                .map(|i| {
                    let a = -TAU * i as f64 / size as f64;
                    (a.cos(), a.sin())
                })
                .collect(),
            window: (0..size)
                .map(|i| 0.5 - 0.5 * (TAU * i as f64 / (size - 1) as f64).cos())
                .collect(),
            real: vec![0.0; size],
            imag: vec![0.0; size],
        }
    }
    fn compute(&mut self, input: &[f32]) -> Vec<f64> {
        for (i, sample) in input.iter().enumerate() {
            self.real[self.permutation[i]] = f64::from(*sample) * self.window[i];
        }
        self.imag.fill(0.0);
        let mut width = 2;
        while width <= self.size {
            let half = width / 2;
            for start in (0..self.size).step_by(width) {
                for j in 0..half {
                    let (c, s) = self.roots[j * self.size / width];
                    let left = start + j;
                    let right = left + half;
                    let r = self.real[right] * c - self.imag[right] * s;
                    let im = self.real[right] * s + self.imag[right] * c;
                    self.real[right] = self.real[left] - r;
                    self.imag[right] = self.imag[left] - im;
                    self.real[left] += r;
                    self.imag[left] += im;
                }
            }
            width *= 2;
        }
        (0..=self.size / 2)
            .map(|i| self.real[i].hypot(self.imag[i]))
            .collect()
    }
}

struct Analyzer {
    samples: usize,
    window: Vec<f32>,
    cursor: usize,
    onset_fft: Spectrum,
    key_fft: Spectrum,
    previous: Vec<f64>,
    flux: Vec<f64>,
    bass: Vec<f64>,
    chroma: [f64; 12],
    pitched_frames: usize,
    waveform: Vec<u8>,
    wave_bin: usize,
    wave_peak: f32,
    energy: f64,
    hop_energy: f64,
    previous_rms: f64,
    attacks: Vec<(f64, f64)>,
}
impl Analyzer {
    fn new() -> Self {
        Self {
            samples: 0,
            window: vec![0.0; KEY_SIZE],
            cursor: 0,
            onset_fft: Spectrum::new(ONSET_SIZE),
            key_fft: Spectrum::new(KEY_SIZE),
            previous: vec![0.0; ONSET_SIZE / 2 + 1],
            flux: Vec::new(),
            bass: Vec::new(),
            chroma: [0.0; 12],
            pitched_frames: 0,
            waveform: Vec::new(),
            wave_bin: 0,
            wave_peak: 0.0,
            energy: 0.0,
            hop_energy: 0.0,
            previous_rms: 0.0,
            attacks: Vec::new(),
        }
    }
    fn push(&mut self, sample: f32, record: bool) {
        if record {
            let bin = self.samples * 50 / RATE;
            if bin != self.wave_bin {
                self.waveform
                    .push((self.wave_peak.clamp(0.0, 1.0).sqrt() * 255.0).round() as u8);
                self.wave_peak = 0.0;
                self.wave_bin = bin;
            }
            self.wave_peak = self.wave_peak.max(sample.abs());
            self.energy += f64::from(sample).powi(2);
            self.hop_energy += f64::from(sample).powi(2);
        }
        self.window[self.cursor] = sample;
        self.cursor = (self.cursor + 1) % KEY_SIZE;
        self.samples += 1;
        if record && self.samples % HOP == 0 {
            let rms = (self.hop_energy / HOP as f64).sqrt();
            let attack = (rms - self.previous_rms).max(0.0);
            if attack > 1e-5 {
                self.attacks
                    .push(((self.samples - HOP / 2) as f64 / RATE as f64, attack));
            }
            self.previous_rms = rms;
            self.hop_energy = 0.0;
        }
        if self.samples >= ONSET_SIZE / 2 && self.samples % HOP == 0 {
            let input: Vec<_> = (0..ONSET_SIZE)
                .map(|i| self.window[(self.cursor + KEY_SIZE - ONSET_SIZE + i) % KEY_SIZE])
                .collect();
            let magnitudes = self.onset_fft.compute(&input);
            let mut flux = 0.0;
            let mut bass = 0.0;
            for (bin, &value) in magnitudes.iter().enumerate().skip(2) {
                let compressed = (1.0 + value * 8.0).ln();
                let change = (compressed - self.previous[bin]).max(0.0);
                let hz = bin as f64 * RATE as f64 / ONSET_SIZE as f64;
                if hz <= 5000.0 {
                    flux += change;
                }
                if hz <= 220.0 {
                    bass += change;
                }
                self.previous[bin] = compressed;
            }
            self.flux.push(flux);
            self.bass.push(bass);
        }
        if self.samples >= KEY_SIZE && self.samples % 2048 == 0 {
            let input: Vec<_> = (0..KEY_SIZE)
                .map(|i| self.window[(self.cursor + i) % KEY_SIZE])
                .collect();
            let magnitudes = self.key_fft.compute(&input);
            let mut frame = [0.0; 12];
            for bin in 30..(KEY_SIZE / 2 - 2) {
                let m = magnitudes[bin];
                if m < 0.01 || m <= magnitudes[bin - 1] || m <= magnitudes[bin + 1] {
                    continue;
                }
                let (a, b, c) = (
                    magnitudes[bin - 1].max(1e-12).ln(),
                    m.ln(),
                    magnitudes[bin + 1].max(1e-12).ln(),
                );
                let shift = (0.5 * (a - c) / (a - 2.0 * b + c)).clamp(-0.5, 0.5);
                let hz = (bin as f64 + shift) * RATE as f64 / KEY_SIZE as f64;
                let midi = 69.0 + 12.0 * (hz / 440.0).log2();
                if !(40.0..=96.0).contains(&midi) || (midi - midi.round()).abs() > 0.4 {
                    continue;
                }
                let local = magnitudes[bin.saturating_sub(4)..=(bin + 4)]
                    .iter()
                    .sum::<f64>()
                    / 9.0;
                // Whiten local spectral peaks, soften loud harmonics, and favor
                // lower fundamentals without treating loudness as a key label.
                let weight = (m / local.max(1e-9)).min(8.0) * m.sqrt() / hz.powf(0.35);
                frame[(midi.round() as i32).rem_euclid(12) as usize] += weight;
            }
            let total = frame.iter().sum::<f64>();
            if total > 1e-6 {
                for (sum, value) in self.chroma.iter_mut().zip(frame) {
                    *sum += value / total;
                }
                self.pitched_frames += 1;
            }
        }
    }
    fn finish(mut self, min_bpm: f64, max_bpm: f64) -> MusicAnalysis {
        let samples = self.samples;
        self.waveform
            .push((self.wave_peak.clamp(0.0, 1.0).sqrt() * 255.0).round() as u8);
        for _ in 0..ONSET_SIZE / 2 {
            self.push(0.0, false);
        }
        let duration = samples as f64 / RATE as f64;
        let rms = (self.energy / samples as f64).sqrt();
        let (mut grid, tempo_confidence, downbeat_confidence) =
            detect_grid(&self.flux, &self.bass, duration, min_bpm, max_bpm);
        if let Some(grid) = grid.as_mut() {
            // Spectral-change maxima can precede a physical transient because
            // of the analysis window. Refine the phase from short RMS rises on
            // the actual timeline, without moving the fitted beat interval.
            let period = 60.0 / grid.bpm;
            let mut offsets: Vec<_> = self
                .attacks
                .iter()
                .filter_map(|&(time, weight)| {
                    let offset = (time - grid.anchor_seconds + period / 2.0).rem_euclid(period)
                        - period / 2.0;
                    (offset.abs() < 0.06_f64.min(period * 0.15)).then_some((offset, weight))
                })
                .collect();
            offsets.sort_by(|a, b| a.0.total_cmp(&b.0));
            let half_weight = offsets.iter().map(|v| v.1).sum::<f64>() / 2.0;
            let mut accumulated = 0.0;
            for (offset, weight) in offsets {
                accumulated += weight;
                if accumulated >= half_weight {
                    let adjusted = grid.anchor_seconds + offset;
                    if adjusted < 0.0 {
                        grid.beat_in_bar = grid.beat_in_bar % 4 + 1;
                    } else if adjusted >= period {
                        grid.beat_in_bar = (grid.beat_in_bar + 2) % 4 + 1;
                    }
                    grid.anchor_seconds = adjusted.rem_euclid(period);
                    break;
                }
            }
        }
        let (key, key_confidence) = detect_key(self.chroma, self.pitched_frames);
        let mut warnings = vec![
            "Grilla de tempo constante en 4/4; el análisis no sigue cambios de tempo.".into(),
            "El primer tiempo del compás es una estimación. Revisa la grilla antes de exportar."
                .into(),
        ];
        if grid.is_none() {
            warnings.push("No se encontró un pulso suficientemente claro. Puedes crear la grilla manualmente.".into());
        }
        if key.is_none() {
            warnings.push("No se encontró una tonalidad suficientemente clara; se conserva la metadata existente.".into());
        }
        if rms < 1e-5 {
            warnings.push(
                "El audio es silencioso o tiene un nivel demasiado bajo para el análisis.".into(),
            );
        }
        MusicAnalysis {
            version: VERSION.into(),
            duration_seconds: duration,
            bpm: grid.as_ref().map(|g| g.bpm),
            anchor_seconds: grid.as_ref().map(|g| g.anchor_seconds),
            beat_in_bar: grid.as_ref().map_or(1, |g| g.beat_in_bar),
            tempo_confidence,
            downbeat_confidence,
            key,
            key_confidence,
            waveform_hz: 50,
            waveform: self.waveform,
            warnings,
        }
    }
}

fn detect_grid(
    flux: &[f64],
    bass: &[f64],
    duration: f64,
    min: f64,
    max: f64,
) -> (Option<GridCorrection>, f64, f64) {
    let rate = RATE as f64 / HOP as f64;
    let radius = (rate / 2.0) as usize;
    let mut sums = vec![0.0; flux.len() + 1];
    for (i, value) in flux.iter().enumerate() {
        sums[i + 1] = sums[i] + value;
    }
    let novelty: Vec<_> = flux
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let left = i.saturating_sub(radius);
            let right = (i + radius + 1).min(flux.len());
            (v - (sums[right] - sums[left]) / (right - left) as f64).max(0.0)
        })
        .collect();
    let peak = novelty.iter().copied().fold(0.0, f64::max);
    let mut events = Vec::<(f64, f64, f64)>::new();
    for i in 1..novelty.len().saturating_sub(1) {
        let value = novelty[i];
        if value < peak * 0.025 || value < 1e-6 || value < novelty[i - 1] || value <= novelty[i + 1]
        {
            continue;
        }
        // First spectral frame is centered at t=0; timestamps stay on the
        // decoded timeline, including silence before the first actual attack.
        let time = i as f64 / rate;
        if time >= duration || events.last().is_some_and(|e| time - e.0 < 0.055) {
            continue;
        }
        events.push((time, value.sqrt(), bass[i]));
    }
    if duration < 8.0 || events.len() < 8 {
        return (None, 0.0, 0.0);
    }
    let total = events.iter().map(|e| e.1).sum::<f64>();
    let fit = |bpm: f64| -> (f64, f64, f64) {
        let period = 60.0 / bpm;
        let mut bins = [0.0; 96];
        for &(time, weight, _) in &events {
            let bin = ((time / period).fract() * 96.0) as usize % 96;
            bins[bin] += weight;
        }
        let (index, mass) = (0..96)
            .map(|i| (i, bins[(i + 95) % 96] + bins[i] + bins[(i + 1) % 96]))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        let concentration = mass / total;
        // Mild octave preference only breaks close pulse/half-pulse ties.
        let preference = 1.0 - 0.06 * (bpm / 120.0).log2().abs();
        (
            concentration * preference,
            (index as f64 + 0.5) / 96.0 * period,
            concentration,
        )
    };
    let mut best = (0.0_f64, min, 0.0, 0.0);
    for index in 0..=((max - min) / 0.2).ceil() as usize {
        let bpm = (min + index as f64 * 0.2).min(max);
        let (score, phase, concentration) = fit(bpm);
        if score > best.0 {
            best = (score, bpm, phase, concentration);
        }
    }
    let coarse = best.1;
    for index in -40..=40 {
        let bpm = coarse + index as f64 * 0.005;
        if bpm < min || bpm > max {
            continue;
        }
        let (score, phase, concentration) = fit(bpm);
        if score > best.0 {
            best = (score, bpm, phase, concentration);
        }
    }
    if best.3 < 0.12 {
        return (None, 0.0, 0.0);
    }
    let bpm = (best.1 * 100.0).round() / 100.0;
    let period = 60.0 / bpm;
    // Refine phase from actual nearby events, allowing the folded peak to
    // straddle zero, rather than keeping its quantized histogram-bin center.
    let mut weighted_offset = 0.0;
    let mut weights = 0.0;
    for &(time, weight, _) in &events {
        let offset = (time - best.2 + period / 2.0).rem_euclid(period) - period / 2.0;
        if offset.abs() < period * 0.05 {
            weighted_offset += offset * weight;
            weights += weight;
        }
    }
    let phase = (best.2 + weighted_offset / weights.max(1e-9)).rem_euclid(period);
    let mut accents = [0.0; 4];
    let mut counts = [0.0_f64; 4];
    for &(time, _, low) in &events {
        let beat = ((time - phase) / period).round();
        if (time - (phase + beat * period)).abs() <= period * 0.12 {
            let slot = (beat as i64).rem_euclid(4) as usize;
            accents[slot] += low.sqrt();
            counts[slot] += 1.0;
        }
    }
    let averages = std::array::from_fn::<_, 4, _>(|i| accents[i] / counts[i].max(1.0));
    let mut ranked: Vec<_> = averages.iter().copied().enumerate().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let downbeat_confidence = ((ranked[0].1 - ranked[1].1) / ranked[0].1.max(1e-9)).clamp(0.0, 1.0);
    let beat_in_bar = ((4 - ranked[0].0) % 4 + 1) as u16;
    (
        Some(GridCorrection {
            bpm,
            anchor_seconds: phase,
            beat_in_bar,
        }),
        ((best.3 - 0.1) / 0.65).clamp(0.0, 1.0),
        downbeat_confidence,
    )
}

fn detect_key(chroma: [f64; 12], frames: usize) -> (Option<String>, f64) {
    if frames < 12 {
        return (None, 0.0);
    }
    // Rau's scale templates: tonic, third and fifth have higher weight than
    // the other scale tones. These weights are not a vendor's trained model.
    let major = [4.0, 0.0, 1.5, 0.0, 3.0, 1.5, 0.0, 3.0, 0.0, 1.5, 0.0, 1.5];
    let minor = [4.0, 0.0, 1.5, 3.0, 0.0, 1.5, 0.0, 3.0, 1.5, 0.0, 1.5, 0.0];
    let mean = chroma.iter().sum::<f64>() / 12.0;
    let norm = chroma
        .iter()
        .map(|v| (v - mean).powi(2))
        .sum::<f64>()
        .sqrt();
    if norm < 1e-6 {
        return (None, 0.0);
    }
    let mut candidates = Vec::new();
    for (mode, template) in [major, minor].iter().enumerate() {
        let center = template.iter().sum::<f64>() / 12.0;
        let scale = template
            .iter()
            .map(|v| (v - center).powi(2))
            .sum::<f64>()
            .sqrt();
        for tonic in 0..12 {
            let correlation = (0..12)
                .map(|i| (chroma[(i + tonic) % 12] - mean) * (template[i] - center))
                .sum::<f64>()
                / (norm * scale);
            candidates.push((correlation, tonic, mode));
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let best = candidates[0];
    let margin = best.0 - candidates[1].0;
    if best.0 < 0.45 || margin < 0.025 {
        return (None, 0.0);
    }
    let names = [
        "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
    ];
    (
        Some(format!(
            "{}{}",
            names[best.1],
            if best.2 == 1 { "m" } else { "" }
        )),
        (best.0 * (margin / 0.2).min(1.0)).clamp(0.0, 1.0),
    )
}
