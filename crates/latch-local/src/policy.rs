use serde::{Deserialize, Serialize};

use crate::Permissions;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    #[default]
    Deny,
    Ask,
    Allow,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    FilesRead,
    FilesWrite,
    Exec,
    Terminal,
    ApplicationControl,
    UiInspection,
    UiControl,
    ScreenCapture,
    RawInput,
    BrowserIsolated,
    BrowserAuthenticated,
    ClipboardRead,
    ClipboardWrite,
    McpDiscovery,
    McpExecution,
    NativeSystemControl,
}

impl Capability {
    pub const ALL: [Self; 16] = [
        Self::FilesRead,
        Self::FilesWrite,
        Self::Exec,
        Self::Terminal,
        Self::ApplicationControl,
        Self::UiInspection,
        Self::UiControl,
        Self::ScreenCapture,
        Self::RawInput,
        Self::BrowserIsolated,
        Self::BrowserAuthenticated,
        Self::ClipboardRead,
        Self::ClipboardWrite,
        Self::McpDiscovery,
        Self::McpExecution,
        Self::NativeSystemControl,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionPolicy {
    pub files_read: PermissionMode,
    pub files_write: PermissionMode,
    pub exec: PermissionMode,
    pub terminal: PermissionMode,
    pub application_control: PermissionMode,
    pub ui_inspection: PermissionMode,
    pub ui_control: PermissionMode,
    pub screen_capture: PermissionMode,
    pub raw_input: PermissionMode,
    pub browser_isolated: PermissionMode,
    pub browser_authenticated: PermissionMode,
    pub clipboard_read: PermissionMode,
    pub clipboard_write: PermissionMode,
    pub mcp_discovery: PermissionMode,
    pub mcp_execution: PermissionMode,
    pub native_system_control: PermissionMode,
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        Self::from_legacy(&Permissions::default())
    }
}

impl PermissionPolicy {
    pub fn from_legacy(legacy: &Permissions) -> Self {
        let files = mode(legacy.files);
        let commands = mode(legacy.commands);
        let screen = mode(legacy.screen);
        let control = mode(legacy.computer_control);
        Self {
            files_read: files,
            files_write: files,
            exec: commands,
            terminal: if legacy.commands { PermissionMode::Ask } else { PermissionMode::Deny },
            application_control: control,
            ui_inspection: control,
            ui_control: control,
            screen_capture: screen,
            raw_input: control,
            browser_isolated: PermissionMode::Ask,
            browser_authenticated: PermissionMode::Deny,
            clipboard_read: PermissionMode::Ask,
            clipboard_write: PermissionMode::Ask,
            mcp_discovery: mode(legacy.mcp_discovery),
            mcp_execution: mode(legacy.mcp_execution),
            native_system_control: PermissionMode::Ask,
        }
    }

    pub const fn get(&self, capability: Capability) -> PermissionMode {
        match capability {
            Capability::FilesRead => self.files_read,
            Capability::FilesWrite => self.files_write,
            Capability::Exec => self.exec,
            Capability::Terminal => self.terminal,
            Capability::ApplicationControl => self.application_control,
            Capability::UiInspection => self.ui_inspection,
            Capability::UiControl => self.ui_control,
            Capability::ScreenCapture => self.screen_capture,
            Capability::RawInput => self.raw_input,
            Capability::BrowserIsolated => self.browser_isolated,
            Capability::BrowserAuthenticated => self.browser_authenticated,
            Capability::ClipboardRead => self.clipboard_read,
            Capability::ClipboardWrite => self.clipboard_write,
            Capability::McpDiscovery => self.mcp_discovery,
            Capability::McpExecution => self.mcp_execution,
            Capability::NativeSystemControl => self.native_system_control,
        }
    }

    pub fn set(&mut self, capability: Capability, mode: PermissionMode) {
        match capability {
            Capability::FilesRead => self.files_read = mode,
            Capability::FilesWrite => self.files_write = mode,
            Capability::Exec => self.exec = mode,
            Capability::Terminal => self.terminal = mode,
            Capability::ApplicationControl => self.application_control = mode,
            Capability::UiInspection => self.ui_inspection = mode,
            Capability::UiControl => self.ui_control = mode,
            Capability::ScreenCapture => self.screen_capture = mode,
            Capability::RawInput => self.raw_input = mode,
            Capability::BrowserIsolated => self.browser_isolated = mode,
            Capability::BrowserAuthenticated => self.browser_authenticated = mode,
            Capability::ClipboardRead => self.clipboard_read = mode,
            Capability::ClipboardWrite => self.clipboard_write = mode,
            Capability::McpDiscovery => self.mcp_discovery = mode,
            Capability::McpExecution => self.mcp_execution = mode,
            Capability::NativeSystemControl => self.native_system_control = mode,
        }
    }

    pub fn preset(preset: PermissionPreset) -> Self {
        use PermissionMode::{Allow, Ask, Deny};
        match preset {
            PermissionPreset::Observe => Self {
                files_read: Allow,
                files_write: Deny,
                exec: Deny,
                terminal: Deny,
                application_control: Deny,
                ui_inspection: Allow,
                ui_control: Deny,
                screen_capture: Ask,
                raw_input: Deny,
                browser_isolated: Allow,
                browser_authenticated: Deny,
                clipboard_read: Ask,
                clipboard_write: Deny,
                mcp_discovery: Allow,
                mcp_execution: Deny,
                native_system_control: Deny,
            },
            PermissionPreset::Work => Self {
                files_read: Allow,
                files_write: Allow,
                exec: Ask,
                terminal: Ask,
                application_control: Ask,
                ui_inspection: Allow,
                ui_control: Ask,
                screen_capture: Ask,
                raw_input: Ask,
                browser_isolated: Allow,
                browser_authenticated: Deny,
                clipboard_read: Ask,
                clipboard_write: Ask,
                mcp_discovery: Allow,
                mcp_execution: Ask,
                native_system_control: Ask,
            },
            PermissionPreset::Developer => Self {
                files_read: Allow,
                files_write: Allow,
                exec: Allow,
                terminal: Allow,
                application_control: Allow,
                ui_inspection: Allow,
                ui_control: Ask,
                screen_capture: Allow,
                raw_input: Ask,
                browser_isolated: Allow,
                browser_authenticated: Ask,
                clipboard_read: Ask,
                clipboard_write: Ask,
                mcp_discovery: Allow,
                mcp_execution: Ask,
                native_system_control: Ask,
            },
            PermissionPreset::FullControl => Self {
                files_read: Allow,
                files_write: Allow,
                exec: Allow,
                terminal: Allow,
                application_control: Allow,
                ui_inspection: Allow,
                ui_control: Allow,
                screen_capture: Allow,
                raw_input: Allow,
                browser_isolated: Allow,
                browser_authenticated: Allow,
                clipboard_read: Allow,
                clipboard_write: Allow,
                mcp_discovery: Allow,
                mcp_execution: Allow,
                native_system_control: Allow,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPreset {
    Observe,
    Work,
    Developer,
    FullControl,
}

const fn mode(enabled: bool) -> PermissionMode {
    if enabled {
        PermissionMode::Allow
    } else {
        PermissionMode::Deny
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_permissions_migrate_without_escalating_power() {
        let legacy = Permissions::default();
        let policy = PermissionPolicy::from_legacy(&legacy);
        assert_eq!(policy.files_read, PermissionMode::Allow);
        assert_eq!(policy.files_write, PermissionMode::Allow);
        assert_eq!(policy.exec, PermissionMode::Allow);
        assert_eq!(policy.raw_input, PermissionMode::Deny);
        assert_eq!(policy.browser_authenticated, PermissionMode::Deny);
    }

    #[test]
    fn developer_preset_keeps_ambient_authority_sensitive_operations_ask() {
        let policy = PermissionPolicy::preset(PermissionPreset::Developer);
        assert_eq!(policy.terminal, PermissionMode::Allow);
        assert_eq!(policy.raw_input, PermissionMode::Ask);
        assert_eq!(policy.native_system_control, PermissionMode::Ask);
        assert_eq!(policy.browser_authenticated, PermissionMode::Ask);
    }
}
