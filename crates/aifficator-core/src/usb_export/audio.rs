use super::{invalid, AudioFormat, AudioInfo, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Read RIFF PCM metadata from the bytes produced, not from an XML duration or
/// filename. Skip unrelated chunks without loading audio into memory.
pub fn read_pcm_wave(path: &Path) -> Result<AudioInfo> {
    let mut file = File::open(path)?;
    Ok(wave_layout(&mut file)?.0)
}

fn wave_layout(file: &mut File) -> Result<(AudioInfo, u64)> {
    file.seek(SeekFrom::Start(0))?;
    let size = file.metadata()?.len();
    let mut header = [0; 12];
    file.read_exact(&mut header)?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" || size > u32::MAX as u64 {
        return Err(invalid("expected a RIFF/WAVE file smaller than 4 GiB"));
    }
    let declared = u32::from_le_bytes(header[4..8].try_into().unwrap()) as u64 + 8;
    if declared != size {
        return Err(invalid("RIFF length does not match file size"));
    }
    let mut format = None;
    let mut data_bytes = None;
    let mut data_offset = 0;
    let mut at = 12u64;
    while at + 8 <= size {
        file.seek(SeekFrom::Start(at))?;
        let mut chunk = [0; 8];
        file.read_exact(&mut chunk)?;
        let length = u32::from_le_bytes(chunk[4..].try_into().unwrap()) as u64;
        if at + 8 + length > size {
            return Err(invalid("truncated WAVE chunk"));
        }
        match &chunk[..4] {
            b"fmt " => {
                if format.is_some() || length < 16 {
                    return Err(invalid("invalid WAVE format chunk"));
                }
                let mut bytes = [0; 16];
                file.read_exact(&mut bytes)?;
                let word = |i| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
                if word(0) != 1 {
                    return Err(invalid(
                        "only integer PCM WAVE is supported in this prototype",
                    ));
                }
                let sample_rate = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
                let byte_rate = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
                let channels = word(2);
                let depth = word(14);
                let alignment = word(12);
                if !matches!(channels, 1 | 2)
                    || !matches!(depth, 16 | 24)
                    || !matches!(sample_rate, 44_100 | 48_000)
                    || u32::from(alignment) != u32::from(channels) * u32::from(depth) / 8
                    || byte_rate != sample_rate * u32::from(alignment)
                {
                    return Err(invalid("unsupported or inconsistent PCM parameters"));
                }
                format = Some((sample_rate, channels, depth, alignment));
            }
            b"data" if data_bytes.is_some() => return Err(invalid("multiple WAVE data chunks")),
            b"data" => {
                data_bytes = Some(length);
                data_offset = at + 8;
            }
            _ => {}
        }
        at += 8 + length + length % 2;
    }
    if at != size {
        return Err(invalid("trailing or truncated WAVE bytes"));
    }
    let (sample_rate, channels, bit_depth, alignment) =
        format.ok_or_else(|| invalid("WAVE has no format chunk"))?;
    let data = data_bytes.ok_or_else(|| invalid("WAVE has no audio chunk"))?;
    if data == 0 || data % u64::from(alignment) != 0 {
        return Err(invalid("incomplete or empty PCM frames"));
    }
    let frames = data / u64::from(alignment);
    if frames.div_ceil(u64::from(sample_rate)) > u16::MAX as u64 {
        return Err(invalid("audio duration exceeds DeviceSQL field width"));
    }
    Ok((
        AudioInfo {
            format: AudioFormat::Wave,
            bitrate_kbps: sample_rate * u32::from(channels) * u32::from(bit_depth) / 1000,
            sample_rate,
            channels,
            bit_depth,
            frames,
            size_bytes: size as u32,
        },
        data_offset,
    ))
}

pub(super) struct Waveforms {
    pub preview: Vec<u8>,
    pub tiny: Vec<u8>,
    pub detail: Vec<u8>,
    pub color_preview: Vec<u8>,
    pub color_detail: Vec<u8>,
    pub three_band_preview: Vec<u8>,
    pub three_band_detail: Vec<u8>,
}

// Rau's own visualization, not a reconstruction of Rekordbox's analyzer. Two
// one-pole crossovers separate low/mid/high energy at 250 and 2,500 Hz. Each
// channel has independent state; opposite-phase stereo must remain visible.
#[derive(Clone, Copy, Default)]
struct BandFilter {
    dc: f64,
    low: f64,
    below_high: f64,
}

impl BandFilter {
    fn process(&mut self, sample: f64, rates: &[f64; 3]) -> [f64; 3] {
        self.dc += rates[0] * (sample - self.dc);
        let centered = sample - self.dc;
        self.low += rates[1] * (centered - self.low);
        self.below_high += rates[2] * (centered - self.below_high);
        [
            self.low,
            self.below_high - self.low,
            centered - self.below_high,
        ]
    }
}

#[derive(Clone, Copy, Default)]
struct ColorColumn {
    peak: f64,
    energy: [f64; 3],
    frames: u64,
}

fn level(value: f64, ceiling: u8) -> u8 {
    (value.clamp(0.0, 1.0) * f64::from(ceiling)).round() as u8
}

impl ColorColumn {
    fn add(&mut self, peak: f64, energy: [f64; 3]) {
        self.peak = self.peak.max(peak);
        for (total, value) in self.energy.iter_mut().zip(energy) {
            *total += value;
        }
        self.frames += 1;
    }

    fn relative_bands(&self) -> [f64; 3] {
        let rms = self.energy.map(|e| (e / self.frames.max(1) as f64).sqrt());
        let strongest = rms.into_iter().fold(0.0, f64::max);
        // Avoid assigning arbitrary saturated colors to numerical filter tails.
        if strongest < 1.0e-6 || self.peak == 0.0 {
            [0.0; 3]
        } else {
            rms.map(|v| v / strongest)
        }
    }

    fn preview(&self) -> [u8; 6] {
        if self.peak == 0.0 {
            return [0; 6];
        }
        let bands = self.relative_bands();
        let height = self.peak.sqrt();
        // PWV4 wire fields: raw peak, blue brightness, blue height, R/G/B.
        // The brightness flag/range matches local reference framing. Our
        // height and color curves are deliberately simple and independently
        // chosen; visual equivalence to Rekordbox is not claimed.
        [
            level(self.peak, 127),
            128 | level(bands[2], 127),
            level(height, 127),
            level(height * bands[0], 127),
            level(height * bands[1], 127),
            level(height * bands[2], 127),
        ]
    }

    fn detail(&self) -> [u8; 2] {
        let [red, green, blue] = self.relative_bands().map(|b| u16::from(level(b, 7)));
        let height = u16::from(level(self.peak.sqrt(), 31));
        ((red << 13) | (green << 10) | (blue << 7) | (height << 2)).to_be_bytes()
    }

    fn three_band(&self, preview: bool) -> [u8; 3] {
        if self.peak == 0.0 {
            return [0; 3];
        }
        let rms = self.energy.map(|e| (e / self.frames.max(1) as f64).sqrt());
        // PWV6 draws stacked heights; cap their combined range. PWV7 draws
        // each band on the same axis. These are Rau's own display curves.
        let total = rms.iter().sum::<f64>().max(1.0);
        let [low, mid, high] = rms.map(|r| {
            if preview {
                (r / total * 127.0).floor().clamp(0.0, 127.0) as u8
            } else {
                level(r.sqrt(), 127)
            }
        });
        [mid, high, low]
    }
}

/// Independent amplitude visualization: maximum absolute sample across channels,
/// square-root height mapping and the original blue fallback. RGB companions
/// add filtered energy without changing the legacy waveform payloads.
pub(super) fn waveforms(path: &Path, expected: &AudioInfo) -> Result<Waveforms> {
    let mut file = File::open(path)?;
    let (info, offset) = wave_layout(&mut file)?;
    if &info != expected {
        return Err(invalid("audio changed before waveform generation"));
    }
    let point_count = (info.frames * 150).div_ceil(u64::from(info.sample_rate));
    let mut detail = Vec::with_capacity(point_count as usize);
    let mut color_detail = Vec::with_capacity(point_count as usize * 2);
    let mut three_band_detail = Vec::with_capacity(point_count as usize * 3);
    let mut color_preview = vec![ColorColumn::default(); 1200];
    let mut filters = [BandFilter::default(); 2];
    let rates = [20.0, 250.0, 2500.0]
        .map(|hz| 1.0 - (-std::f64::consts::TAU * hz / f64::from(info.sample_rate)).exp());
    let mut preview = [0u32; 400];
    let mut tiny = [0u32; 100];
    let sample_bytes = usize::from(info.bit_depth / 8);
    let frame_bytes = sample_bytes * usize::from(info.channels);
    let full_scale = (1u32 << (info.bit_depth - 1)) as f64;
    let height = |peak: u32, ceiling: u8, floor: u8| {
        ((f64::from(peak) / full_scale).sqrt() * f64::from(ceiling))
            .round()
            .clamp(f64::from(floor), f64::from(ceiling)) as u8
    };
    file.seek(SeekFrom::Start(offset))?;
    // Divisible by every supported frame size (2, 4, 3, 6).
    let mut buffer = [0u8; 65_532];
    let mut frame = 0u64;
    let mut bin = 0;
    let mut bin_peak = 0;
    let mut bin_color = ColorColumn::default();
    while frame < info.frames {
        let frames = (info.frames - frame).min((buffer.len() / frame_bytes) as u64) as usize;
        file.read_exact(&mut buffer[..frames * frame_bytes])?;
        for samples in buffer[..frames * frame_bytes].chunks_exact(frame_bytes) {
            let next_bin = frame * 150 / u64::from(info.sample_rate);
            if next_bin != bin {
                detail.push(height(bin_peak, 31, 0));
                color_detail.extend(bin_color.detail());
                three_band_detail.extend(bin_color.three_band(false));
                bin_peak = 0;
                bin_color = ColorColumn::default();
                bin = next_bin;
            }
            let mut peak = 0;
            let mut energy = [0.0_f64; 3];
            for (channel, sample) in samples.chunks_exact(sample_bytes).enumerate() {
                let signed = if sample_bytes == 2 {
                    i32::from(i16::from_le_bytes([sample[0], sample[1]]))
                } else {
                    i32::from_le_bytes([0, sample[0], sample[1], sample[2]]) >> 8
                };
                peak = peak.max(signed.unsigned_abs());
                let bands = filters[channel].process(f64::from(signed) / full_scale, &rates);
                for (power, band) in energy.iter_mut().zip(bands) {
                    *power = power.max(band * band);
                }
            }
            bin_peak = bin_peak.max(peak);
            bin_color.add(f64::from(peak) / full_scale, energy);
            let color_index = (frame * 1200 / info.frames) as usize;
            color_preview[color_index].add(f64::from(peak) / full_scale, energy);
            let p = (frame * 400 / info.frames) as usize;
            let t = (frame * 100 / info.frames) as usize;
            preview[p] = preview[p].max(peak);
            tiny[t] = tiny[t].max(peak);
            frame += 1;
        }
    }
    detail.push(height(bin_peak, 31, 0));
    color_detail.extend(bin_color.detail());
    three_band_detail.extend(bin_color.three_band(false));
    if detail.len() != point_count as usize {
        return Err(invalid(
            "waveform column count differs from decoded duration",
        ));
    }
    Ok(Waveforms {
        preview: preview.into_iter().map(|p| height(p, 25, 2)).collect(),
        tiny: tiny.into_iter().map(|p| height(p, 15, 1)).collect(),
        detail,
        color_preview: color_preview
            .iter()
            .flat_map(ColorColumn::preview)
            .collect(),
        color_detail,
        three_band_preview: color_preview
            .iter()
            .flat_map(|c| c.three_band(true))
            .collect(),
        three_band_detail,
    })
}
