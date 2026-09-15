use std::collections::HashMap;

use thiserror::Error;
use uuid::Uuid;

const MAX_SCREENSHOT_BYTES: usize = 8 * 1024 * 1024;
const MAX_TYPE_CHARS: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayInfo {
    pub display_id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotFormat {
    Jpeg,
    WebP,
    Png,
}

impl ScreenshotFormat {
    pub const fn mime_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::WebP => "image/webp",
            Self::Png => "image/png",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screenshot {
    pub display_id: String,
    pub width: u32,
    pub height: u32,
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub window_id: String,
    pub title: String,
    pub process_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub focused: bool,
    pub minimized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Default)]
pub struct ComputerManager {
    windows: HashMap<String, u32>,
}

impl ComputerManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn displays(&self) -> Result<Vec<DisplayInfo>, ComputerError> {
        platform::displays()
    }

    pub fn screenshot(
        &self,
        display_id: Option<&str>,
        format: ScreenshotFormat,
    ) -> Result<Screenshot, ComputerError> {
        platform::screenshot(display_id, format)
    }

    pub fn windows(&mut self) -> Result<Vec<WindowInfo>, ComputerError> {
        let raw = platform::windows()?;
        self.windows.clear();
        let mut result = Vec::with_capacity(raw.len());
        for window in raw {
            let opaque = Uuid::new_v4().to_string();
            self.windows.insert(opaque.clone(), window.native_id);
            result.push(WindowInfo {
                window_id: opaque,
                title: window.title,
                process_name: window.process_name,
                x: window.x,
                y: window.y,
                width: window.width,
                height: window.height,
                focused: window.focused,
                minimized: window.minimized,
            });
        }
        Ok(result)
    }

    pub fn focus(&self, window_id: &str) -> Result<(), ComputerError> {
        let native = self
            .windows
            .get(window_id)
            .copied()
            .ok_or(ComputerError::WindowNotFound)?;
        platform::focus(native)
    }

    pub fn mouse_move(&self, x: i32, y: i32) -> Result<(), ComputerError> {
        self.validate_point(x, y)?;
        platform::mouse_move(x, y)
    }

    pub fn mouse_click(&self, button: MouseButton) -> Result<(), ComputerError> {
        platform::mouse_click(button)
    }

    pub fn mouse_drag(
        &self,
        from_x: i32,
        from_y: i32,
        to_x: i32,
        to_y: i32,
        button: MouseButton,
    ) -> Result<(), ComputerError> {
        self.validate_point(from_x, from_y)?;
        self.validate_point(to_x, to_y)?;
        platform::mouse_drag(from_x, from_y, to_x, to_y, button)
    }

    pub fn scroll(&self, amount: i32, axis: ScrollAxis) -> Result<(), ComputerError> {
        if amount.unsigned_abs() > 10_000 {
            return Err(ComputerError::InvalidInput(
                "scroll amount must be between -10000 and 10000".to_owned(),
            ));
        }
        platform::scroll(amount, axis)
    }

    pub fn key(&self, key: &str, modifiers: &[String]) -> Result<(), ComputerError> {
        if modifiers.len() > 4 {
            return Err(ComputerError::InvalidInput(
                "at most four modifiers are allowed".to_owned(),
            ));
        }
        platform::key(key, modifiers)
    }

    pub fn type_text(&self, text: &str) -> Result<(), ComputerError> {
        if text.chars().count() > MAX_TYPE_CHARS {
            return Err(ComputerError::InvalidInput(format!(
                "typed text exceeds {MAX_TYPE_CHARS} characters"
            )));
        }
        platform::type_text(text)
    }

    fn validate_point(&self, x: i32, y: i32) -> Result<(), ComputerError> {
        let valid = self.displays()?.iter().any(|display| {
            let right = i64::from(display.x) + i64::from(display.width);
            let bottom = i64::from(display.y) + i64::from(display.height);
            i64::from(x) >= i64::from(display.x)
                && i64::from(x) < right
                && i64::from(y) >= i64::from(display.y)
                && i64::from(y) < bottom
        });
        if valid {
            Ok(())
        } else {
            Err(ComputerError::InvalidInput(
                "coordinates are outside the current displays".to_owned(),
            ))
        }
    }
}

#[derive(Debug, Error)]
pub enum ComputerError {
    #[error("computer use is only supported on Windows in this release")]
    UnsupportedPlatform,
    #[error("window id is unknown or expired")]
    WindowNotFound,
    #[error("invalid computer input: {0}")]
    InvalidInput(String),
    #[error("screenshot exceeds the {MAX_SCREENSHOT_BYTES} byte transport limit")]
    ScreenshotTooLarge,
    #[error("computer operation failed: {0}")]
    Operation(String),
}

#[derive(Debug)]
struct RawWindow {
    native_id: u32,
    title: String,
    process_name: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    focused: bool,
    minimized: bool,
}

#[cfg(windows)]
mod platform {
    use std::{ffi::c_void, io::Cursor, thread, time::Duration};

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use enigo::{
        Axis, Button, Coordinate,
        Direction::{Click, Press, Release},
        Enigo, Key, Keyboard, Mouse, Settings,
    };
    use image::{codecs::jpeg::JpegEncoder, DynamicImage, ImageFormat};
    use winsafe::{prelude::*, HWND};
    use xcap::{Monitor, Window};

    use super::{
        ComputerError, DisplayInfo, MouseButton, RawWindow, Screenshot, ScreenshotFormat,
        ScrollAxis, MAX_SCREENSHOT_BYTES,
    };

    pub fn displays() -> Result<Vec<DisplayInfo>, ComputerError> {
        Monitor::all()
            .map_err(operation)?
            .into_iter()
            .map(|monitor| {
                let native_id = monitor.id().map_err(operation)?;
                Ok(DisplayInfo {
                    display_id: format!("display-{native_id}"),
                    name: monitor
                        .friendly_name()
                        .unwrap_or_else(|_| format!("Display {native_id}")),
                    x: monitor.x().map_err(operation)?,
                    y: monitor.y().map_err(operation)?,
                    width: monitor.width().map_err(operation)?,
                    height: monitor.height().map_err(operation)?,
                    scale_factor: monitor.scale_factor().map_err(operation)?,
                    primary: monitor.is_primary().map_err(operation)?,
                })
            })
            .collect()
    }

    pub fn screenshot(
        display_id: Option<&str>,
        format: ScreenshotFormat,
    ) -> Result<Screenshot, ComputerError> {
        let monitors = Monitor::all().map_err(operation)?;
        let monitor = match display_id {
            Some(display_id) => monitors
                .into_iter()
                .find(|monitor| {
                    monitor
                        .id()
                        .is_ok_and(|id| format!("display-{id}") == display_id)
                })
                .ok_or_else(|| {
                    ComputerError::InvalidInput("display id was not found".to_owned())
                })?,
            None => monitors
                .iter()
                .find(|monitor| monitor.is_primary().unwrap_or(false))
                .cloned()
                .or_else(|| monitors.into_iter().next())
                .ok_or_else(|| ComputerError::Operation("no displays are available".to_owned()))?,
        };
        let native_id = monitor.id().map_err(operation)?;
        let image = monitor.capture_image().map_err(operation)?;
        let width = image.width();
        let height = image.height();
        let mut output = Cursor::new(Vec::new());
        match format {
            ScreenshotFormat::Jpeg => {
                JpegEncoder::new_with_quality(&mut output, 75)
                    .encode_image(&DynamicImage::ImageRgba8(image))
                    .map_err(operation)?;
            }
            ScreenshotFormat::WebP => DynamicImage::ImageRgba8(image)
                .write_to(&mut output, ImageFormat::WebP)
                .map_err(operation)?,
            ScreenshotFormat::Png => DynamicImage::ImageRgba8(image)
                .write_to(&mut output, ImageFormat::Png)
                .map_err(operation)?,
        }
        let bytes = output.into_inner();
        if bytes.len() > MAX_SCREENSHOT_BYTES {
            return Err(ComputerError::ScreenshotTooLarge);
        }
        Ok(Screenshot {
            display_id: format!("display-{native_id}"),
            width,
            height,
            mime_type: format.mime_type().to_owned(),
            data_base64: STANDARD.encode(bytes),
        })
    }

    pub fn windows() -> Result<Vec<RawWindow>, ComputerError> {
        Window::all()
            .map_err(operation)?
            .into_iter()
            .filter_map(|window| {
                let title = window.title().ok()?;
                if title.trim().is_empty() {
                    return None;
                }
                Some(Ok(RawWindow {
                    native_id: window.id().map_err(operation)?,
                    title,
                    process_name: window.app_name().unwrap_or_default(),
                    x: window.x().map_err(operation)?,
                    y: window.y().map_err(operation)?,
                    width: window.width().map_err(operation)?,
                    height: window.height().map_err(operation)?,
                    focused: window.is_focused().unwrap_or(false),
                    minimized: window.is_minimized().unwrap_or(false),
                }))
            })
            .collect()
    }

    #[allow(unsafe_code)]
    pub fn focus(native_id: u32) -> Result<(), ComputerError> {
        let pointer =
            usize::try_from(native_id).map_err(|_| ComputerError::WindowNotFound)? as *mut c_void;
        let window = unsafe { HWND::from_ptr(pointer) };
        if window.SetForegroundWindow() {
            Ok(())
        } else {
            Err(ComputerError::Operation(
                "Windows refused to focus the selected window".to_owned(),
            ))
        }
    }

    pub fn mouse_move(x: i32, y: i32) -> Result<(), ComputerError> {
        enigo()?
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(operation)
    }

    pub fn mouse_click(button: MouseButton) -> Result<(), ComputerError> {
        enigo()?
            .button(button_value(button), Click)
            .map_err(operation)
    }

    pub fn mouse_drag(
        from_x: i32,
        from_y: i32,
        to_x: i32,
        to_y: i32,
        button: MouseButton,
    ) -> Result<(), ComputerError> {
        let mut input = enigo()?;
        input
            .move_mouse(from_x, from_y, Coordinate::Abs)
            .map_err(operation)?;
        input
            .button(button_value(button), Press)
            .map_err(operation)?;
        for step in 1..=20 {
            let x = from_x + (to_x - from_x) * step / 20;
            let y = from_y + (to_y - from_y) * step / 20;
            input.move_mouse(x, y, Coordinate::Abs).map_err(operation)?;
            thread::sleep(Duration::from_millis(5));
        }
        input
            .button(button_value(button), Release)
            .map_err(operation)
    }

    pub fn scroll(amount: i32, axis: ScrollAxis) -> Result<(), ComputerError> {
        let axis = match axis {
            ScrollAxis::Vertical => Axis::Vertical,
            ScrollAxis::Horizontal => Axis::Horizontal,
        };
        enigo()?.scroll(amount, axis).map_err(operation)
    }

    pub fn key(key: &str, modifiers: &[String]) -> Result<(), ComputerError> {
        let mut input = enigo()?;
        let modifiers = modifiers
            .iter()
            .map(|value| key_value(value))
            .collect::<Result<Vec<_>, _>>()?;
        for modifier in &modifiers {
            input.key(*modifier, Press).map_err(operation)?;
        }
        input.key(key_value(key)?, Click).map_err(operation)?;
        for modifier in modifiers.iter().rev() {
            input.key(*modifier, Release).map_err(operation)?;
        }
        Ok(())
    }

    pub fn type_text(text: &str) -> Result<(), ComputerError> {
        enigo()?.text(text).map_err(operation)
    }

    fn enigo() -> Result<Enigo, ComputerError> {
        Enigo::new(&Settings::default()).map_err(operation)
    }

    const fn button_value(button: MouseButton) -> Button {
        match button {
            MouseButton::Left => Button::Left,
            MouseButton::Right => Button::Right,
            MouseButton::Middle => Button::Middle,
        }
    }

    fn key_value(value: &str) -> Result<Key, ComputerError> {
        let lower = value.to_ascii_lowercase();
        let key = match lower.as_str() {
            "ctrl" | "control" => Key::Control,
            "alt" => Key::Alt,
            "shift" => Key::Shift,
            "meta" | "win" | "windows" => Key::Meta,
            "enter" | "return" => Key::Return,
            "tab" => Key::Tab,
            "escape" | "esc" => Key::Escape,
            "space" => Key::Space,
            "backspace" => Key::Backspace,
            "delete" => Key::Delete,
            "up" => Key::UpArrow,
            "down" => Key::DownArrow,
            "left" => Key::LeftArrow,
            "right" => Key::RightArrow,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            "f1" => Key::F1,
            "f2" => Key::F2,
            "f3" => Key::F3,
            "f4" => Key::F4,
            "f5" => Key::F5,
            "f6" => Key::F6,
            "f7" => Key::F7,
            "f8" => Key::F8,
            "f9" => Key::F9,
            "f10" => Key::F10,
            "f11" => Key::F11,
            "f12" => Key::F12,
            _ => {
                let mut chars = value.chars();
                let character = chars.next().ok_or_else(|| {
                    ComputerError::InvalidInput("key must not be empty".to_owned())
                })?;
                if chars.next().is_some() {
                    return Err(ComputerError::InvalidInput(format!(
                        "unsupported key: {value}"
                    )));
                }
                Key::Unicode(character)
            }
        };
        Ok(key)
    }

    fn operation(error: impl std::fmt::Display) -> ComputerError {
        ComputerError::Operation(error.to_string())
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{
        ComputerError, DisplayInfo, MouseButton, RawWindow, Screenshot, ScreenshotFormat,
        ScrollAxis,
    };

    pub fn displays() -> Result<Vec<DisplayInfo>, ComputerError> {
        Err(ComputerError::UnsupportedPlatform)
    }
    pub fn screenshot(
        _display_id: Option<&str>,
        _format: ScreenshotFormat,
    ) -> Result<Screenshot, ComputerError> {
        Err(ComputerError::UnsupportedPlatform)
    }
    pub fn windows() -> Result<Vec<RawWindow>, ComputerError> {
        Err(ComputerError::UnsupportedPlatform)
    }
    pub fn focus(_native_id: u32) -> Result<(), ComputerError> {
        Err(ComputerError::UnsupportedPlatform)
    }
    pub fn mouse_move(_x: i32, _y: i32) -> Result<(), ComputerError> {
        Err(ComputerError::UnsupportedPlatform)
    }
    pub fn mouse_click(_button: MouseButton) -> Result<(), ComputerError> {
        Err(ComputerError::UnsupportedPlatform)
    }
    pub fn mouse_drag(
        _from_x: i32,
        _from_y: i32,
        _to_x: i32,
        _to_y: i32,
        _button: MouseButton,
    ) -> Result<(), ComputerError> {
        Err(ComputerError::UnsupportedPlatform)
    }
    pub fn scroll(_amount: i32, _axis: ScrollAxis) -> Result<(), ComputerError> {
        Err(ComputerError::UnsupportedPlatform)
    }
    pub fn key(_key: &str, _modifiers: &[String]) -> Result<(), ComputerError> {
        Err(ComputerError::UnsupportedPlatform)
    }
    pub fn type_text(_text: &str) -> Result<(), ComputerError> {
        Err(ComputerError::UnsupportedPlatform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_mime_types_are_stable() {
        assert_eq!(ScreenshotFormat::Jpeg.mime_type(), "image/jpeg");
        assert_eq!(ScreenshotFormat::WebP.mime_type(), "image/webp");
        assert_eq!(ScreenshotFormat::Png.mime_type(), "image/png");
    }

    #[test]
    fn unknown_window_ids_are_rejected_before_platform_calls() {
        assert!(matches!(
            ComputerManager::new().focus("not-a-window"),
            Err(ComputerError::WindowNotFound)
        ));
    }
}
