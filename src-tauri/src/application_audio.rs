use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const APPLICATION_AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const SYSTEM_AUDIO_TARGET_ID: &str = "__system_audio__";
pub const SYSTEM_AUDIO_LABEL: &str = "Salida completa del Mac";
pub const APPLICATION_AUDIO_MINIMUM_MACOS_VERSION: &str = "13.0";
#[cfg(target_os = "macos")]
const APPLICATION_AUDIO_BUFFER_SECONDS: usize = 2;
#[cfg(any(target_os = "macos", test))]
const APPLICATION_AUDIO_UNSUPPORTED_MESSAGE: &str =
    "La captura de la salida del Mac requiere macOS Ventura 13 o posterior. En Monterey puedes usar micrófono o entrada de línea.";

#[derive(Debug, Clone)]
pub struct ApplicationAudioDevice {
    pub id: String,
    pub label: String,
    pub process_id: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplicationAudioSupport {
    pub supported: bool,
    pub minimum_macos_version: String,
    pub current_macos_version: Option<String>,
    pub message: String,
}

pub struct ApplicationAudioCapture {
    #[cfg(target_os = "macos")]
    stream: screencapturekit::stream::SCStream,
}

pub struct ApplicationAudioCaptureParts {
    pub capture: ApplicationAudioCapture,
    pub buffer: Arc<Mutex<VecDeque<[i16; 2]>>>,
    pub stream_error: Arc<Mutex<Option<String>>>,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Eq, PartialEq)]
struct MacosVersion {
    major: u32,
    minor: u32,
    patch: u32,
    display: String,
}

#[cfg(any(target_os = "macos", test))]
fn parse_macos_version(value: &str) -> Option<MacosVersion> {
    let display = value.trim();
    let mut components = display.split('.');
    let major = components.next()?.parse().ok()?;
    let minor = components.next().unwrap_or("0").parse().ok()?;
    let patch = components.next().unwrap_or("0").parse().ok()?;
    Some(MacosVersion {
        major,
        minor,
        patch,
        display: display.to_string(),
    })
}

#[cfg(any(target_os = "macos", test))]
fn support_for_macos_version(version: Option<MacosVersion>) -> ApplicationAudioSupport {
    let supported = version.as_ref().is_some_and(|version| version.major >= 13);
    ApplicationAudioSupport {
        supported,
        minimum_macos_version: APPLICATION_AUDIO_MINIMUM_MACOS_VERSION.to_string(),
        current_macos_version: version.map(|version| version.display),
        message: if supported {
            String::new()
        } else {
            APPLICATION_AUDIO_UNSUPPORTED_MESSAGE.to_string()
        },
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use core_foundation::error::CFError;
    use core_media_rs::cm_sample_buffer::CMSampleBuffer;
    use screencapturekit::{
        shareable_content::SCShareableContent,
        stream::{
            configuration::SCStreamConfiguration, content_filter::SCContentFilter,
            delegate_trait::SCStreamDelegateTrait, output_trait::SCStreamOutputTrait,
            output_type::SCStreamOutputType, SCStream,
        },
    };
    use std::process::Command;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    struct AudioHandler {
        buffer: Arc<Mutex<VecDeque<[i16; 2]>>>,
        maximum_frames: usize,
    }

    impl SCStreamOutputTrait for AudioHandler {
        fn did_output_sample_buffer(
            &self,
            sample: CMSampleBuffer,
            output_type: SCStreamOutputType,
        ) {
            if output_type != SCStreamOutputType::Audio {
                return;
            }
            let Ok(audio_buffers) = sample.get_audio_buffer_list() else {
                return;
            };
            let Ok(mut target) = self.buffer.lock() else {
                return;
            };
            append_float32_audio(&audio_buffers, &mut target);
            if target.len() > self.maximum_frames {
                let excess = target.len() - self.maximum_frames;
                target.drain(..excess);
            }
        }
    }

    struct StreamDelegate {
        stream_error: Arc<Mutex<Option<String>>>,
    }

    impl SCStreamDelegateTrait for StreamDelegate {
        fn did_stop_with_error(&self, _stream: SCStream, error: CFError) {
            if let Ok(mut target) = self.stream_error.lock() {
                *target = Some(format!("La captura de audio del Mac se detuvo: {error}"));
            }
        }
    }

    pub fn support() -> ApplicationAudioSupport {
        let version = Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                let value = String::from_utf8(output.stdout).ok()?;
                parse_macos_version(&value)
            });
        support_for_macos_version(version)
    }

    pub fn list_applications() -> Result<Vec<ApplicationAudioDevice>, String> {
        ensure_supported()?;
        let content = shareable_content()?;
        let own_process_id = std::process::id() as i32;
        let mut applications = content
            .applications()
            .into_iter()
            .filter_map(|application| {
                let id = application.bundle_identifier();
                let label = application.application_name();
                let process_id = application.process_id();
                (!id.trim().is_empty() && !label.trim().is_empty() && process_id != own_process_id)
                    .then_some(ApplicationAudioDevice {
                        id,
                        label,
                        process_id,
                    })
            })
            .collect::<Vec<_>>();
        applications.sort_by(|left, right| {
            left.label
                .to_lowercase()
                .cmp(&right.label.to_lowercase())
                .then_with(|| left.process_id.cmp(&right.process_id))
        });
        applications.dedup_by(|left, right| left.id == right.id);
        Ok(applications)
    }

    pub fn start_capture(target_id: &str) -> Result<ApplicationAudioCaptureParts, String> {
        ensure_supported()?;
        let content = shareable_content()?;
        let displays = content.displays();
        let display = displays
            .first()
            .ok_or_else(|| "No hay una pantalla disponible para capturar audio.".to_string())?;
        let filter = if target_id == SYSTEM_AUDIO_TARGET_ID {
            SCContentFilter::new().with_display_excluding_applications_excepting_windows(
                display,
                &[],
                &[],
            )
        } else {
            let applications = content.applications();
            let application = applications
                .iter()
                .find(|application| application.bundle_identifier() == target_id)
                .ok_or_else(|| {
                    "La aplicación seleccionada ya no está abierta o disponible.".to_string()
                })?;
            SCContentFilter::new().with_display_including_application_excepting_windows(
                display,
                &[application],
                &[],
            )
        };
        let configuration = SCStreamConfiguration::new()
            .set_width(2)
            .map_err(configuration_error)?
            .set_height(2)
            .map_err(configuration_error)?
            .set_queue_depth(1)
            .map_err(configuration_error)?
            .set_shows_cursor(false)
            .map_err(configuration_error)?
            .set_captures_audio(true)
            .map_err(configuration_error)?
            .set_excludes_current_process_audio(true)
            .map_err(configuration_error)?
            .set_sample_rate(APPLICATION_AUDIO_SAMPLE_RATE)
            .map_err(configuration_error)?
            .set_channel_count(2)
            .map_err(configuration_error)?;
        let buffer = Arc::new(Mutex::new(VecDeque::new()));
        let stream_error = Arc::new(Mutex::new(None));
        let delegate = StreamDelegate {
            stream_error: Arc::clone(&stream_error),
        };
        let mut stream = SCStream::new_with_delegate(&filter, &configuration, delegate);
        if stream
            .add_output_handler(
                AudioHandler {
                    buffer: Arc::clone(&buffer),
                    maximum_frames: APPLICATION_AUDIO_SAMPLE_RATE as usize
                        * APPLICATION_AUDIO_BUFFER_SECONDS,
                },
                SCStreamOutputType::Audio,
            )
            .is_none()
        {
            return Err("No se pudo registrar la salida de audio del Mac.".to_string());
        }
        stream.start_capture().map_err(|error| {
            format!(
                "No se pudo capturar el audio del Mac. Revisa el permiso de Grabación de pantalla y audio para Rau Studio: {error}"
            )
        })?;
        Ok(ApplicationAudioCaptureParts {
            capture: ApplicationAudioCapture { stream },
            buffer,
            stream_error,
        })
    }

    pub fn open_permission_settings() -> Result<(), String> {
        ensure_supported()?;
        Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("No se pudieron abrir los ajustes de privacidad: {error}"))
    }

    fn ensure_supported() -> Result<(), String> {
        let support = support();
        support.supported.then_some(()).ok_or(support.message)
    }

    fn configuration_error(error: CFError) -> String {
        format!("No se pudo configurar la captura de audio del Mac: {error}")
    }

    fn shareable_content() -> Result<SCShareableContent, String> {
        let granted = unsafe { CGPreflightScreenCaptureAccess() }
            || unsafe { CGRequestScreenCaptureAccess() };
        if !granted {
            return Err(
                "macOS no autorizó la captura. Activa Rau Studio en Privacidad y seguridad → Grabación de pantalla y audio del sistema, cierra completamente la app y vuelve a abrirla."
                    .to_string(),
            );
        }
        SCShareableContent::get().map_err(|error| {
            format!(
                "No se pudieron consultar aplicaciones. Autoriza Grabación de pantalla y audio para Rau Studio y vuelve a intentarlo: {error}"
            )
        })
    }

    fn append_float32_audio(
        audio_buffers: &core_audio_types_rs::audio_buffer_list::AudioBufferList,
        target: &mut VecDeque<[i16; 2]>,
    ) {
        if audio_buffers.num_buffers() >= 2 {
            let Some(left) = audio_buffers.get(0) else {
                return;
            };
            let Some(right) = audio_buffers.get(1) else {
                return;
            };
            for (left, right) in float32_samples(left.data()).zip(float32_samples(right.data())) {
                target.push_back([float_to_i16(left), float_to_i16(right)]);
            }
            return;
        }

        let Some(buffer) = audio_buffers.get(0) else {
            return;
        };
        let channels = usize::try_from(buffer.number_channels).unwrap_or(0);
        if channels == 0 {
            return;
        }
        let samples = float32_samples(buffer.data()).collect::<Vec<_>>();
        for frame in samples.chunks_exact(channels) {
            let left = float_to_i16(frame[0]);
            let right = if channels > 1 {
                float_to_i16(frame[1])
            } else {
                left
            };
            target.push_back([left, right]);
        }
    }

    fn float32_samples(bytes: &[u8]) -> impl Iterator<Item = f32> + '_ {
        bytes
            .chunks_exact(4)
            .map(|sample| f32::from_ne_bytes([sample[0], sample[1], sample[2], sample[3]]))
    }

    fn float_to_i16(sample: f32) -> i16 {
        (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
    }

    impl ApplicationAudioCapture {
        pub fn stop(self) {
            let _ = self.stream.stop_capture();
        }
    }
}

#[cfg(target_os = "macos")]
pub use platform::{list_applications, open_permission_settings, start_capture, support};

#[cfg(not(target_os = "macos"))]
pub fn support() -> ApplicationAudioSupport {
    ApplicationAudioSupport {
        supported: false,
        minimum_macos_version: APPLICATION_AUDIO_MINIMUM_MACOS_VERSION.to_string(),
        current_macos_version: None,
        message: "La captura de audio del sistema solo está disponible en macOS.".to_string(),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn list_applications() -> Result<Vec<ApplicationAudioDevice>, String> {
    Err("La captura de audio del sistema solo está disponible en macOS.".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn start_capture(_bundle_id: &str) -> Result<ApplicationAudioCaptureParts, String> {
    Err("La captura de audio del sistema solo está disponible en macOS.".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn open_permission_settings() -> Result<(), String> {
    Err("Los ajustes de captura de audio del sistema solo están disponibles en macOS.".to_string())
}

#[cfg(not(target_os = "macos"))]
impl ApplicationAudioCapture {
    pub fn stop(self) {}
}

#[cfg(test)]
mod tests {
    use super::{
        parse_macos_version, support_for_macos_version, MacosVersion, SYSTEM_AUDIO_LABEL,
        SYSTEM_AUDIO_TARGET_ID,
    };

    #[test]
    fn float_pcm_conversion_is_clamped() {
        let convert = |sample: f32| (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        assert_eq!(convert(2.0), i16::MAX);
        assert_eq!(convert(-2.0), -i16::MAX);
        assert_eq!(convert(0.0), 0);
    }

    #[test]
    fn system_audio_target_has_a_stable_persisted_id() {
        assert_eq!(SYSTEM_AUDIO_TARGET_ID, "__system_audio__");
        assert_eq!(SYSTEM_AUDIO_LABEL, "Salida completa del Mac");
    }

    #[test]
    fn parses_macos_versions_with_optional_components() {
        assert_eq!(
            parse_macos_version("12.7.6\n"),
            Some(MacosVersion {
                major: 12,
                minor: 7,
                patch: 6,
                display: "12.7.6".to_string(),
            })
        );
        assert_eq!(
            parse_macos_version("13"),
            Some(MacosVersion {
                major: 13,
                minor: 0,
                patch: 0,
                display: "13".to_string(),
            })
        );
        assert_eq!(parse_macos_version("not-a-version"), None);
    }

    #[test]
    fn application_audio_requires_ventura() {
        let monterey = support_for_macos_version(parse_macos_version("12.7.6"));
        assert!(!monterey.supported);
        assert_eq!(monterey.current_macos_version.as_deref(), Some("12.7.6"));

        let ventura = support_for_macos_version(parse_macos_version("13.0"));
        assert!(ventura.supported);
        assert!(ventura.message.is_empty());

        let unknown = support_for_macos_version(None);
        assert!(!unknown.supported);
    }
}
