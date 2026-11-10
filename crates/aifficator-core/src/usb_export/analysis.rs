//! Analysis shared by device database formats. Timing comes from XML or an
//! explicitly saved Rau preparation; AverageBpm alone is never a grid input.

use super::{invalid, AudioInfo, Result};
use crate::rekordbox::Track;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TrackAnalysis {
    /// Bind an explicitly prepared grid/key to the analyzed source bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    pub tempos: Vec<TempoAnchor>,
    pub cues: Vec<CuePoint>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TempoAnchor {
    pub seconds: f64,
    pub bpm: f64,
    pub beat_in_bar: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CuePoint {
    /// 0 is a memory cue; 1..=16 are hot cues A..P.
    pub hot_cue: u8,
    pub start_ms: u32,
    pub end_ms: Option<u32>,
    pub name: String,
    /// Explicit RGB, exported by rekordbox-usb without guessing a palette index.
    pub rgb: Option<[u8; 3]>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Beat {
    pub beat_in_bar: u16,
    pub bpm_x100: u16,
    pub time_ms: u32,
}

fn field<'a>(attrs: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str> {
    attrs
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| invalid(format!("missing analysis attribute {key}")))
}

fn decimal(attrs: &BTreeMap<String, String>, key: &str) -> Result<f64> {
    let value: f64 = field(attrs, key)?
        .parse()
        .map_err(|_| invalid(format!("invalid analysis attribute {key}")))?;
    if !value.is_finite() || value < 0.0 {
        return Err(invalid(format!(
            "analysis {key} must be a finite nonnegative number"
        )));
    }
    Ok(value)
}

fn milliseconds(seconds: f64) -> Result<u32> {
    let value = (seconds * 1000.0).round();
    if !value.is_finite() || !(0.0..f64::from(u32::MAX)).contains(&value) {
        return Err(invalid("analysis time exceeds millisecond field range"));
    }
    Ok(value as u32)
}

pub fn from_xml(track: &Track) -> Result<TrackAnalysis> {
    let mut analysis = TrackAnalysis::default();
    for attrs in &track.tempos {
        if field(attrs, "Metro")? != "4/4" {
            return Err(invalid(
                "this analysis writer currently requires a 4/4 XML grid",
            ));
        }
        analysis.tempos.push(TempoAnchor {
            seconds: decimal(attrs, "Inizio")?,
            bpm: decimal(attrs, "Bpm")?,
            beat_in_bar: field(attrs, "Battito")?
                .parse()
                .map_err(|_| invalid("invalid Battito"))?,
        });
    }
    analysis
        .tempos
        .sort_by(|a, b| a.seconds.total_cmp(&b.seconds));
    if analysis.tempos.is_empty() {
        analysis.warnings.push(
            "No XML beat grid: no timing grid or quantization data was invented from AverageBpm"
                .into(),
        );
    }
    for attrs in &track.position_marks {
        let kind = field(attrs, "Type")?;
        if matches!(kind, "1" | "2" | "3") {
            analysis.warnings.push(format!("XML position mark type {kind} is preserved in the library but has no device cue mapping in this prototype"));
            continue;
        }
        if !matches!(kind, "0" | "4") {
            return Err(invalid(format!("unsupported position mark Type {kind}")));
        }
        let slot: i16 = field(attrs, "Num")?
            .parse()
            .map_err(|_| invalid("invalid cue Num"))?;
        if !(-1..=15).contains(&slot) {
            return Err(invalid("cue Num must be -1 (memory) or 0..15 (hot cue)"));
        }
        let start = decimal(attrs, "Start")?;
        let end = if kind == "4" {
            Some(decimal(attrs, "End")?)
        } else {
            None
        };
        let color = ["Red", "Green", "Blue"];
        let rgb = if color.iter().any(|key| attrs.contains_key(*key)) {
            let mut values = [0; 3];
            for (i, key) in color.iter().enumerate() {
                values[i] = field(attrs, key)?
                    .parse()
                    .map_err(|_| invalid("cue RGB component must be 0..255"))?;
            }
            Some(values)
        } else {
            None
        };
        analysis.cues.push(CuePoint {
            hot_cue: (slot + 1) as u8,
            start_ms: milliseconds(start)?,
            end_ms: end.map(milliseconds).transpose()?,
            name: attrs.get("Name").cloned().unwrap_or_default(),
            rgb,
        });
    }
    analysis.cues.sort_by_key(|c| (c.hot_cue, c.start_ms));
    #[cfg(not(unix))]
    if analysis.cues.iter().any(|cue| cue.rgb.is_some()) {
        analysis.warnings.push(
            "Cue RGB values are retained in the report; custom cue colors are not written yet"
                .into(),
        );
    }
    validate(&analysis)?;
    Ok(analysis)
}

pub fn validate(analysis: &TrackAnalysis) -> Result<()> {
    if analysis
        .source_sha256
        .as_ref()
        .is_some_and(|hash| hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(invalid("invalid musical analysis source hash"));
    }
    if analysis.tempos.len() > 100_000 || analysis.cues.len() > 26 {
        return Err(invalid("analysis exceeds prototype marker limits"));
    }
    let mut previous = None;
    for tempo in &analysis.tempos {
        if !tempo.seconds.is_finite()
            || tempo.seconds < 0.0
            || !tempo.bpm.is_finite()
            || !(0.01..=655.35).contains(&tempo.bpm)
            || !(1..=4).contains(&tempo.beat_in_bar)
            || previous.is_some_and(|p| tempo.seconds <= p)
        {
            return Err(invalid(
                "grid requires increasing nonnegative times, BPM 0.01..655.35 and beats 1..4",
            ));
        }
        milliseconds(tempo.seconds)?;
        previous = Some(tempo.seconds);
    }
    let mut slots = BTreeSet::new();
    let mut memory = 0;
    for cue in &analysis.cues {
        if cue.hot_cue > 16
            || (cue.hot_cue > 0 && !slots.insert(cue.hot_cue))
            || cue
                .end_ms
                .is_some_and(|end| end <= cue.start_ms || end == u32::MAX)
            || cue.start_ms == u32::MAX
            || cue.name.contains('\0')
            || cue.name.len() > 1024
        {
            return Err(invalid(
                "invalid cue time, duplicate hot slot, or invalid cue name",
            ));
        }
        memory += usize::from(cue.hot_cue == 0);
    }
    if memory > 10 {
        return Err(invalid(
            "prototype supports at most ten memory cues/loops per track",
        ));
    }
    Ok(())
}

/// Expand each XML anchor independently. Absolute multiplication avoids drift
/// from repeatedly rounding/adding an integer beat interval. Changes of tempo
/// and bar position take effect at the next anchor, even between former beats.
/// Segment ends are exclusive at the serialized millisecond precision: a beat
/// that rounds to the next anchor belongs to that next anchor. If several
/// anchors round to one millisecond, the last anchor takes precedence there.
pub fn beats(analysis: &TrackAnalysis, audio: &AudioInfo) -> Result<Vec<Beat>> {
    validate(analysis)?;
    if audio.sample_rate == 0 || audio.frames == 0 {
        return Err(invalid("grid needs a nonempty decoded audio timeline"));
    }
    let duration = audio.frames as f64 / f64::from(audio.sample_rate);
    let end_ms = milliseconds(duration)?;
    for cue in &analysis.cues {
        if cue.start_ms >= end_ms || cue.end_ms.is_some_and(|end| end > end_ms) {
            return Err(invalid(
                "cue or loop lies outside the decoded audio timeline",
            ));
        }
    }
    let mut result: Vec<Beat> = Vec::new();
    for (index, anchor) in analysis.tempos.iter().enumerate() {
        if anchor.seconds >= duration {
            return Err(invalid(
                "grid anchor lies outside the decoded audio timeline",
            ));
        }
        let until = analysis
            .tempos
            .get(index + 1)
            .map_or(duration, |a| a.seconds);
        let until_ms = milliseconds(until)?.min(end_ms);
        let period = 60.0 / anchor.bpm;
        let count = ((until - anchor.seconds) / period).ceil() as usize;
        if count > 1_000_000 || result.len().saturating_add(count) > 1_000_000 {
            return Err(invalid("grid exceeds one million beats"));
        }
        for offset in 0..count {
            let seconds = anchor.seconds + offset as f64 * period;
            if seconds >= until {
                break;
            }
            let time_ms = milliseconds(seconds)?;
            // Comparing seconds alone lets a beat just before `until` round
            // to the same timestamp as the next anchor. Stop this segment at
            // that boundary so the next anchor supplies BPM and bar position.
            if time_ms >= until_ms {
                break;
            }
            if result.last().is_some_and(|b| b.time_ms >= time_ms) {
                return Err(invalid(
                    "grid anchors produce overlapping beats at millisecond precision",
                ));
            }
            result.push(Beat {
                beat_in_bar: (anchor.beat_in_bar - 1 + (offset % 4) as u16) % 4 + 1,
                bpm_x100: (anchor.bpm * 100.0).round() as u16,
                time_ms,
            });
        }
    }
    Ok(result)
}
