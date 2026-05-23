//! Unified DeepSeek TUI theme tokens.
//!
//! Merges the former `palette::UiTheme` (chrome colors) and
//! `deepseek_theme::Theme` (section/tool/plan colors) into a single struct
//! that covers every visual site in the TUI.  Built-in presets inherit from
//! both sources and can be overridden field-by-field via `Settings`.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{BorderType, Borders, Padding};

use crate::palette;
use crate::palette::PaletteMode;
use crate::tui::history::ToolStatus;

// ── Unified Theme struct ────────────────────────────────────────────────────

/// Centralized visual tokens for every rendered surface in the TUI.
///
/// Includes the former `UiTheme` chrome colours and the former
/// `deepseek_theme::Theme` section/tool/plan colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    // ── Chrome colours (ex palette::UiTheme) ──
    pub name: &'static str,
    pub mode: PaletteMode,
    pub surface_bg: Color,
    pub panel_bg: Color,
    pub sidebar_bg: Color, // NEW — defaults to panel_bg
    pub elevated_bg: Color,
    pub composer_bg: Color,
    pub selection_bg: Color,
    pub header_bg: Color,
    pub footer_bg: Color,
    pub mode_agent: Color,
    pub mode_yolo: Color,
    pub mode_plan: Color,
    pub status_ready: Color,
    pub status_working: Color,
    pub status_warning: Color,
    pub text_dim: Color,
    pub text_hint: Color,
    pub text_muted: Color,
    pub text_body: Color,
    pub text_soft: Color,
    pub border_color: Color,
    pub border_type: BorderType,         // NEW — Plain | Rounded
    pub section_border_type: BorderType, // NEW — defaults to border_type

    // ── Section / tool / plan colours (ex deepseek_theme::Theme) ──
    pub section_borders: Borders,
    pub section_border_color: Color,
    pub section_title_color: Color,
    pub section_padding: Padding,
    pub tool_title_color: Color,
    pub tool_value_color: Color,
    pub tool_label_color: Color,
    pub tool_running_accent: Color,
    pub tool_success_accent: Color,
    pub tool_failed_accent: Color,
    pub plan_progress_color: Color,
    pub plan_summary_color: Color,
    pub plan_explanation_color: Color,
    pub plan_pending_color: Color,
    pub plan_in_progress_color: Color,
    pub plan_completed_color: Color,
    /// Work panel checklist/strategy step symbols. Configurable via
    /// custom theme files (`work_pending_symbol`, etc.).
    pub work_pending_symbol: &'static str,
    pub work_in_progress_symbol: &'static str,
    pub work_completed_symbol: &'static str,
    /// Failed/error status symbol (e.g. "[!]", "✗").
    pub work_failed_symbol: &'static str,
    /// Canceled / interrupted status symbol (e.g. "[-]", "⊘").
    pub work_canceled_symbol: &'static str,
    /// Reasoning / thinking block background tint.  `None` means "use the
    /// built-in hardcoded tint" (`palette::SURFACE_REASONING_TINT`).
    /// Set via theme file `reasoning_bg = "#RRGGBB"` or `"reset"`.
    pub reasoning_bg: Option<Color>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse a border type from a settings string.
#[must_use]
pub fn border_type_from_setting(value: &str) -> BorderType {
    match value.trim().to_ascii_lowercase().as_str() {
        "rounded" => BorderType::Rounded,
        _ => BorderType::Plain,
    }
}

// ── Custom theme file support ───────────────────────────────────────────────

use serde::{Deserialize, Serialize};

/// On-disk representation of a custom theme file.
///
/// Lives under `~/.config/deepseek/themes/<name>.toml`.  Only `base` is
/// required; every other key is optional and overrides the corresponding
/// slot in the base theme.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CustomThemeFile {
    /// Built-in theme to inherit from (e.g. `"dark"`, `"light"`).
    pub base: String,
    /// Main TUI background colour (`#RRGGBB`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    /// Sidebar panel background colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_bg: Option<String>,
    /// Composer input-area background colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composer_bg: Option<String>,
    /// Border style: `"plain"` or `"rounded"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_type: Option<String>,
    /// Reasoning block background tint (`#RRGGBB` or `"reset"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_bg: Option<String>,
    // ── Chrome colours ──
    /// Panel background colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_bg: Option<String>,
    /// Elevated surface background.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevated_bg: Option<String>,
    /// Selection highlight background.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_bg: Option<String>,
    /// Header bar background.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_bg: Option<String>,
    /// Footer bar background.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer_bg: Option<String>,
    /// Agent mode indicator colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_agent: Option<String>,
    /// YOLO mode indicator colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_yolo: Option<String>,
    /// Plan mode indicator colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_plan: Option<String>,
    /// Ready status colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_ready: Option<String>,
    /// Working status colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_working: Option<String>,
    /// Warning status colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_warning: Option<String>,
    // ── Text colours ──
    /// Dimmed text colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_dim: Option<String>,
    /// Hint text colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_hint: Option<String>,
    /// Muted text colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_muted: Option<String>,
    /// Body text colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_body: Option<String>,
    /// Soft text colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_soft: Option<String>,
    /// Border line colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_color: Option<String>,
    // ── Section / tool colours ──
    /// Section border colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_border_color: Option<String>,
    /// Section title colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_title_color: Option<String>,
    /// Tool title colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_title_color: Option<String>,
    /// Tool value colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_value_color: Option<String>,
    /// Tool label colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_label_color: Option<String>,
    /// Running tool accent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_running_accent: Option<String>,
    /// Successful tool accent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_success_accent: Option<String>,
    /// Failed tool accent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_failed_accent: Option<String>,
    // ── Plan colours ──
    /// Plan progress bar colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_progress_color: Option<String>,
    /// Plan summary colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_summary_color: Option<String>,
    /// Plan explanation colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_explanation_color: Option<String>,
    /// Pending plan step colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_pending_color: Option<String>,
    /// In-progress plan step colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_in_progress_color: Option<String>,
    /// Completed plan step colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_completed_color: Option<String>,
    // ── Work panel symbols ──
    /// Pending status symbol (e.g. "[ ]", "○").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_pending_symbol: Option<String>,
    /// In-progress status symbol (e.g. "[~]", "◐").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_in_progress_symbol: Option<String>,
    /// Completed status symbol (e.g. "[x]", "✓").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_completed_symbol: Option<String>,
    /// Failed/error status symbol (e.g. "[!]", "✗").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_failed_symbol: Option<String>,
    /// Canceled / interrupted status symbol (e.g. "[-]", "⊘").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_canceled_symbol: Option<String>,
    /// Extra keys that might be added in the future.
    #[serde(flatten)]
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    pub extras: std::collections::BTreeMap<String, toml::Value>,
}

impl CustomThemeFile {
    /// Serialise to a TOML string for display or saving.
    #[allow(dead_code)]
    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Write this theme to `~/.config/deepseek/themes/<name>.toml`.
    #[allow(dead_code)]
    pub fn save(&self, name: &str) -> std::io::Result<()> {
        let dir = themes_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{name}.toml"));
        let content = self
            .to_toml_string()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(&path, content)
    }
}

impl Default for CustomThemeFile {
    fn default() -> Self {
        Self {
            base: "dark".to_string(),
            background_color: None,
            sidebar_bg: None,
            composer_bg: None,
            border_type: None,
            reasoning_bg: None,
            panel_bg: None,
            elevated_bg: None,
            selection_bg: None,
            header_bg: None,
            footer_bg: None,
            mode_agent: None,
            mode_yolo: None,
            mode_plan: None,
            status_ready: None,
            status_working: None,
            status_warning: None,
            text_dim: None,
            text_hint: None,
            text_muted: None,
            text_body: None,
            text_soft: None,
            border_color: None,
            section_border_color: None,
            section_title_color: None,
            tool_title_color: None,
            tool_value_color: None,
            tool_label_color: None,
            tool_running_accent: None,
            tool_success_accent: None,
            tool_failed_accent: None,
            plan_progress_color: None,
            plan_summary_color: None,
            plan_explanation_color: None,
            plan_pending_color: None,
            plan_in_progress_color: None,
            plan_completed_color: None,
            work_pending_symbol: None,
            work_in_progress_symbol: None,
            work_completed_symbol: None,
            work_failed_symbol: None,
            work_canceled_symbol: None,
            extras: std::collections::BTreeMap::new(),
        }
    }
}

/// Directory where custom theme files are stored.
#[must_use]
pub fn themes_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("deepseek")
        .join("themes")
}

// ── Theme picker entry ──────────────────────────────────────────────────────

/// A single row in the `/theme` picker.  Either a built-in preset or a
/// custom theme file from `~/.config/deepseek/themes/`.
#[derive(Debug, Clone)]
pub enum ThemePickerEntry {
    /// A built-in named theme (System, Dark, Light).
    Builtin(ThemeId),
    /// A custom theme file loaded from disk.  The string is the file stem
    /// (e.g. `"midnight"` for `midnight.toml`).
    Custom {
        /// File stem used for `file:<stem>` references.
        stem: String,
        /// Display label in the picker (e.g. `"Midnight (custom)"`).
        label: String,
        /// Resolved theme at construction time.
        theme: Box<Theme>,
    },
}

impl ThemePickerEntry {
    /// Settings string for the `theme` key.  Built-ins return the canonical
    /// name (e.g. `"dark"`); custom entries return `"file:<stem>"`.
    #[must_use]
    pub fn setting_name(&self) -> String {
        match self {
            Self::Builtin(id) => id.name().to_string(),
            Self::Custom { stem, .. } => format!("file:{stem}"),
        }
    }

    /// Human-readable label for the picker row.
    #[must_use]
    pub fn display_label(&self) -> &str {
        match self {
            Self::Builtin(id) => id.display_name(),
            Self::Custom { label, .. } => label.as_str(),
        }
    }

    /// Short tagline / description.
    #[must_use]
    pub fn tagline(&self) -> &str {
        match self {
            Self::Builtin(id) => id.tagline(),
            Self::Custom { .. } => "Custom theme file",
        }
    }

    /// Resolve to a concrete `Theme` for live-preview rendering.
    #[must_use]
    pub fn to_theme(&self) -> Theme {
        match self {
            Self::Builtin(id) => id.ui_theme(),
            Self::Custom { theme, .. } => **theme,
        }
    }
}

/// Build the full picker list: built-in themes first, then any `.toml`
/// custom theme files found in `themes_dir()`.
#[must_use]
pub fn build_picker_list() -> Vec<ThemePickerEntry> {
    let mut entries: Vec<ThemePickerEntry> = SELECTABLE_THEMES
        .iter()
        .map(|&id| ThemePickerEntry::Builtin(id))
        .collect();

    let dir = themes_dir();
    if let Ok(read_dir) = std::fs::read_dir(&dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toml")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                // Try to load the file to get a representative Theme for
                // the live-preview swatch.  If parsing fails, skip the
                // entry (broken files shouldn't crash the picker).
                if let Ok(theme) = Theme::from_toml_file(&path) {
                    // Derive a readable label from the stem.
                    let label = stem_to_label(stem);
                    entries.push(ThemePickerEntry::Custom {
                        stem: stem.to_string(),
                        label,
                        theme: Box::new(theme),
                    });
                }
            }
        }
    }

    entries
}

/// Convert a file stem like `"warm-paper"` into a display label like
/// `"Warm Paper (custom)"`.
fn stem_to_label(stem: &str) -> String {
    let pretty: String = stem
        .split(['-', '_'])
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let mut s = c.to_uppercase().collect::<String>();
                    s.push_str(&chars.as_str().to_lowercase());
                    s
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("{pretty} (custom)")
}

// ── ThemeId ──────────────────────────────────────────────────────────────────

/// Stable identifiers for the named themes the user can select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    System,
    Whale,
    WhaleLight,
}

impl ThemeId {
    /// Parse a settings string (`"system"`, `"dark"`, `"catppuccin-mocha"`, …).
    #[must_use]
    pub fn from_name(value: &str) -> Option<Self> {
        match crate::palette::normalize_theme_name(value)? {
            "system" => Some(Self::System),
            "dark" => Some(Self::Whale),
            "light" => Some(Self::WhaleLight),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Whale => "dark",
            Self::WhaleLight => "light",
        }
    }

    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Whale => "Whale (Dark)",
            Self::WhaleLight => "Whale Light",
        }
    }

    #[must_use]
    pub const fn tagline(self) -> &'static str {
        match self {
            Self::System => "Follow terminal background (COLORFGBG)",
            Self::Whale => "Default DeepSeek dark blue",
            Self::WhaleLight => "DeepSeek light, paper-ish",
        }
    }

    /// Resolve to a concrete `Theme`. For `System` this consults
    /// `PaletteMode::detect()` exactly once.
    #[must_use]
    pub fn ui_theme(self) -> Theme {
        match self {
            Self::System => Theme::detect(),
            Self::Whale => DARK_THEME,
            Self::WhaleLight => LIGHT_THEME,
        }
    }
}

/// Themes shown in the `/theme` picker, in display order.
pub const SELECTABLE_THEMES: &[ThemeId] = &[
    ThemeId::System,
    ThemeId::Whale,
    ThemeId::WhaleLight,
];

// ── Built-in Theme constants ─────────────────────────────────────────────────

// Common section defaults used by every built-in preset.
const SECT_BORDERS: Borders = Borders::ALL;
const SECT_PAD: Padding = Padding::horizontal(1);

/// Dark "Whale" theme — the original DeepSeek default.
pub const DARK_THEME: Theme = Theme {
    name: "whale",
    mode: PaletteMode::Dark,
    surface_bg: palette::DEEPSEEK_INK,
    panel_bg: palette::DEEPSEEK_SLATE,
    sidebar_bg: palette::DEEPSEEK_SLATE,
    elevated_bg: palette::SURFACE_ELEVATED,
    composer_bg: palette::DEEPSEEK_SLATE,
    selection_bg: palette::SELECTION_BG,
    header_bg: palette::DEEPSEEK_INK,
    footer_bg: palette::DEEPSEEK_INK,
    mode_agent: palette::MODE_AGENT,
    mode_yolo: palette::MODE_YOLO,
    mode_plan: palette::MODE_PLAN,
    status_ready: palette::TEXT_MUTED,
    status_working: palette::DEEPSEEK_SKY,
    status_warning: palette::STATUS_WARNING,
    text_dim: palette::TEXT_DIM,
    text_hint: palette::TEXT_HINT,
    text_muted: palette::TEXT_MUTED,
    text_body: palette::TEXT_BODY,
    text_soft: palette::TEXT_SOFT,
    border_color: palette::BORDER_COLOR,
    border_type: BorderType::Plain,
    section_border_type: BorderType::Plain,
    section_borders: SECT_BORDERS,
    section_border_color: palette::BORDER_COLOR,
    section_title_color: palette::DEEPSEEK_BLUE,
    section_padding: SECT_PAD,
    tool_title_color: palette::TEXT_SOFT,
    tool_value_color: palette::TEXT_MUTED,
    tool_label_color: palette::TEXT_DIM,
    tool_running_accent: palette::ACCENT_TOOL_LIVE,
    tool_success_accent: palette::TEXT_DIM,
    tool_failed_accent: palette::ACCENT_TOOL_ISSUE,
    plan_progress_color: palette::STATUS_SUCCESS,
    plan_summary_color: palette::TEXT_MUTED,
    plan_explanation_color: palette::TEXT_DIM,
    plan_pending_color: palette::TEXT_MUTED,
    plan_in_progress_color: palette::STATUS_WARNING,
    plan_completed_color: palette::STATUS_SUCCESS,
    work_pending_symbol: "[ ]",
    work_in_progress_symbol: "[~]",
    work_completed_symbol: "[x]",
    work_failed_symbol: "[!]",
    work_canceled_symbol: "[-]",
    reasoning_bg: None,
};

/// Light "Whale Light" theme.
pub const LIGHT_THEME: Theme = Theme {
    name: "whale-light",
    mode: PaletteMode::Light,
    surface_bg: palette::LIGHT_SURFACE,
    panel_bg: palette::LIGHT_PANEL,
    sidebar_bg: palette::LIGHT_PANEL,
    elevated_bg: palette::LIGHT_ELEVATED,
    composer_bg: palette::LIGHT_PANEL,
    selection_bg: palette::LIGHT_SELECTION_BG,
    header_bg: palette::LIGHT_SURFACE,
    footer_bg: palette::LIGHT_SURFACE,
    mode_agent: palette::DEEPSEEK_BLUE,
    mode_yolo: palette::DEEPSEEK_RED,
    mode_plan: Color::Rgb(180, 83, 9),
    status_ready: palette::LIGHT_TEXT_MUTED,
    status_working: palette::DEEPSEEK_BLUE,
    status_warning: Color::Rgb(180, 83, 9),
    text_dim: palette::LIGHT_TEXT_HINT,
    text_hint: palette::LIGHT_TEXT_HINT,
    text_muted: palette::LIGHT_TEXT_MUTED,
    text_body: palette::LIGHT_TEXT_BODY,
    text_soft: palette::LIGHT_TEXT_SOFT,
    border_color: palette::LIGHT_BORDER,
    border_type: BorderType::Plain,
    section_border_type: BorderType::Plain,
    section_borders: SECT_BORDERS,
    section_border_color: palette::LIGHT_BORDER,
    section_title_color: palette::DEEPSEEK_BLUE,
    section_padding: SECT_PAD,
    tool_title_color: palette::LIGHT_TEXT_SOFT,
    tool_value_color: palette::LIGHT_TEXT_MUTED,
    tool_label_color: palette::LIGHT_TEXT_HINT,
    tool_running_accent: palette::DEEPSEEK_BLUE,
    tool_success_accent: palette::LIGHT_TEXT_HINT,
    tool_failed_accent: palette::DEEPSEEK_RED,
    plan_progress_color: palette::DEEPSEEK_BLUE,
    plan_summary_color: palette::LIGHT_TEXT_MUTED,
    plan_explanation_color: palette::LIGHT_TEXT_HINT,
    plan_pending_color: palette::LIGHT_TEXT_MUTED,
    plan_in_progress_color: Color::Rgb(180, 83, 9),
    plan_completed_color: palette::DEEPSEEK_BLUE,
    work_pending_symbol: "[ ]",
    work_in_progress_symbol: "[~]",
    work_completed_symbol: "[x]",
    work_failed_symbol: "[!]",
    work_canceled_symbol: "[-]",
    reasoning_bg: None,
};

/// Apply a single hex-colour override from a `CustomThemeFile` field.
/// If the field is `Some` and parses to a valid colour, calls the
/// corresponding `with_<field>` builder method.
macro_rules! try_override {
    ($theme:ident, $custom:expr, $field:ident, $with:ident) => {
        if let Some(ref c) = $custom.$field {
            if let Some(color) = $crate::palette::parse_hex_rgb_color(c) {
                $theme = $theme.$with(color);
            }
        }
    };
}

// ── Theme methods ────────────────────────────────────────────────────────────

impl Theme {
    #[must_use]
    pub fn for_mode(mode: PaletteMode) -> Self {
        match mode {
            PaletteMode::Dark => DARK_THEME,
            PaletteMode::Light => LIGHT_THEME,
        }
    }

    #[must_use]
    pub fn detect() -> Self {
        Self::for_mode(PaletteMode::detect())
    }

    #[must_use]
    #[allow(dead_code)] // kept for backward compatibility with palette.rs callers
    pub fn from_setting(value: &str) -> Option<Self> {
        ThemeId::from_name(value).map(ThemeId::ui_theme)
    }

    #[must_use]
    pub fn with_background_color(mut self, color: Color) -> Self {
        self.surface_bg = color;
        self.header_bg = color;
        self.footer_bg = color;
        self
    }

    #[must_use]
    pub fn with_sidebar_bg(mut self, color: Color) -> Self {
        self.sidebar_bg = color;
        self
    }

    #[must_use]
    pub fn with_composer_bg(mut self, color: Color) -> Self {
        self.composer_bg = color;
        self
    }

    #[must_use]
    pub fn with_border_type(mut self, bt: BorderType) -> Self {
        self.border_type = bt;
        self
    }

    #[must_use]
    pub fn with_section_border_type(mut self, bt: BorderType) -> Self {
        self.section_border_type = bt;
        self
    }

    #[must_use]
    pub fn with_reasoning_bg(mut self, color: Option<Color>) -> Self {
        self.reasoning_bg = color;
        self
    }

    // ── Chrome colour overrides ──

    #[must_use]
    pub fn with_panel_bg(mut self, color: Color) -> Self {
        self.panel_bg = color;
        self
    }
    #[must_use]
    pub fn with_elevated_bg(mut self, color: Color) -> Self {
        self.elevated_bg = color;
        self
    }
    #[must_use]
    pub fn with_selection_bg(mut self, color: Color) -> Self {
        self.selection_bg = color;
        self
    }
    #[must_use]
    pub fn with_header_bg(mut self, color: Color) -> Self {
        self.header_bg = color;
        self
    }
    #[must_use]
    pub fn with_footer_bg(mut self, color: Color) -> Self {
        self.footer_bg = color;
        self
    }
    #[must_use]
    pub fn with_mode_agent(mut self, color: Color) -> Self {
        self.mode_agent = color;
        self
    }
    #[must_use]
    pub fn with_mode_yolo(mut self, color: Color) -> Self {
        self.mode_yolo = color;
        self
    }
    #[must_use]
    pub fn with_mode_plan(mut self, color: Color) -> Self {
        self.mode_plan = color;
        self
    }
    #[must_use]
    pub fn with_status_ready(mut self, color: Color) -> Self {
        self.status_ready = color;
        self
    }
    #[must_use]
    pub fn with_status_working(mut self, color: Color) -> Self {
        self.status_working = color;
        self
    }
    #[must_use]
    pub fn with_status_warning(mut self, color: Color) -> Self {
        self.status_warning = color;
        self
    }

    // ── Text colour overrides ──

    #[must_use]
    pub fn with_text_dim(mut self, color: Color) -> Self {
        self.text_dim = color;
        self
    }
    #[must_use]
    pub fn with_text_hint(mut self, color: Color) -> Self {
        self.text_hint = color;
        self
    }
    #[must_use]
    pub fn with_text_muted(mut self, color: Color) -> Self {
        self.text_muted = color;
        self
    }
    #[must_use]
    pub fn with_text_body(mut self, color: Color) -> Self {
        self.text_body = color;
        self
    }
    #[must_use]
    pub fn with_text_soft(mut self, color: Color) -> Self {
        self.text_soft = color;
        self
    }

    // ── Border colour override ──

    #[must_use]
    pub fn with_border_color(mut self, color: Color) -> Self {
        self.border_color = color;
        self
    }

    // ── Section / tool colour overrides ──

    #[must_use]
    pub fn with_section_border_color(mut self, color: Color) -> Self {
        self.section_border_color = color;
        self
    }
    #[must_use]
    pub fn with_section_title_color(mut self, color: Color) -> Self {
        self.section_title_color = color;
        self
    }
    #[must_use]
    pub fn with_tool_title_color(mut self, color: Color) -> Self {
        self.tool_title_color = color;
        self
    }
    #[must_use]
    pub fn with_tool_value_color(mut self, color: Color) -> Self {
        self.tool_value_color = color;
        self
    }
    #[must_use]
    pub fn with_tool_label_color(mut self, color: Color) -> Self {
        self.tool_label_color = color;
        self
    }
    #[must_use]
    pub fn with_tool_running_accent(mut self, color: Color) -> Self {
        self.tool_running_accent = color;
        self
    }
    #[must_use]
    pub fn with_tool_success_accent(mut self, color: Color) -> Self {
        self.tool_success_accent = color;
        self
    }
    #[must_use]
    pub fn with_tool_failed_accent(mut self, color: Color) -> Self {
        self.tool_failed_accent = color;
        self
    }

    // ── Plan colour overrides ──

    #[must_use]
    pub fn with_plan_progress_color(mut self, color: Color) -> Self {
        self.plan_progress_color = color;
        self
    }
    #[must_use]
    pub fn with_plan_summary_color(mut self, color: Color) -> Self {
        self.plan_summary_color = color;
        self
    }
    #[must_use]
    pub fn with_plan_explanation_color(mut self, color: Color) -> Self {
        self.plan_explanation_color = color;
        self
    }
    #[must_use]
    pub fn with_plan_pending_color(mut self, color: Color) -> Self {
        self.plan_pending_color = color;
        self
    }
    #[must_use]
    pub fn with_plan_in_progress_color(mut self, color: Color) -> Self {
        self.plan_in_progress_color = color;
        self
    }
    #[must_use]
    pub fn with_plan_completed_color(mut self, color: Color) -> Self {
        self.plan_completed_color = color;
        self
    }

    /// Apply overrides from `Settings`-style optional fields on top of a
    /// resolved base theme.  This is the canonical entry point for building
    /// the runtime `Theme` from `Settings`.
    #[must_use]
    pub fn from_settings(
        theme_setting: &str,
        border_type_setting: Option<&str>,
        section_border_type: Option<&str>,
    ) -> Self {
        let mut t =
            Self::from_setting_or_file(theme_setting).unwrap_or_else(|| ThemeId::System.ui_theme());

        // Only override border_type from settings when the user explicitly
        // configured it.  Custom theme files already set their own.
        if let Some(bt_str) = border_type_setting {
            t = t.with_border_type(border_type_from_setting(bt_str));
        }
        // section_border_type falls back to the (possibly overridden) border_type
        let sbt = section_border_type.map_or(t.border_type, border_type_from_setting);
        t.section_borders = Borders::ALL;
        t = t.with_section_border_type(sbt);
        t
    }

    /// Load a custom theme from a TOML file.
    ///
    /// The file MUST contain a `base` key naming one of the built-in themes.
    /// Any additional keys override the corresponding field of the base theme.
    ///
    /// # Example `~/.config/deepseek/themes/midnight.toml`
    ///
    /// ```toml
    /// base = "tokyo-night"
    /// background_color = "#000000"
    /// sidebar_bg = "#111111"
    /// border_type = "rounded"
    /// ```
    pub fn from_toml_file(path: &std::path::Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let custom: CustomThemeFile = toml::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        Ok(Self::from_custom_theme(&custom))
    }

    /// Build a `Theme` from a deserialized `CustomThemeFile`.
    #[must_use]
    pub fn from_custom_theme(custom: &CustomThemeFile) -> Self {
        let base_id = ThemeId::from_name(&custom.base).unwrap_or(ThemeId::Whale);
        let mut t = base_id.ui_theme();

        try_override!(t, custom, background_color, with_background_color);
        try_override!(t, custom, sidebar_bg, with_sidebar_bg);
        try_override!(t, custom, composer_bg, with_composer_bg);

        if let Some(ref bt) = custom.border_type {
            t = t.with_border_type(border_type_from_setting(bt));
        }
        if let Some(ref c) = custom.reasoning_bg {
            let color = palette::parse_hex_rgb_color(c);
            t = t.with_reasoning_bg(color);
        }

        // ── Apply chrome colour overrides ──
        try_override!(t, custom, panel_bg, with_panel_bg);
        try_override!(t, custom, elevated_bg, with_elevated_bg);
        try_override!(t, custom, selection_bg, with_selection_bg);
        try_override!(t, custom, header_bg, with_header_bg);
        try_override!(t, custom, footer_bg, with_footer_bg);
        try_override!(t, custom, mode_agent, with_mode_agent);
        try_override!(t, custom, mode_yolo, with_mode_yolo);
        try_override!(t, custom, mode_plan, with_mode_plan);
        try_override!(t, custom, status_ready, with_status_ready);
        try_override!(t, custom, status_working, with_status_working);
        try_override!(t, custom, status_warning, with_status_warning);

        // ── Apply text colour overrides ──
        try_override!(t, custom, text_dim, with_text_dim);
        try_override!(t, custom, text_hint, with_text_hint);
        try_override!(t, custom, text_muted, with_text_muted);
        try_override!(t, custom, text_body, with_text_body);
        try_override!(t, custom, text_soft, with_text_soft);

        // ── Apply border colour override ──
        try_override!(t, custom, border_color, with_border_color);

        // ── Apply section/tool colour overrides ──
        try_override!(t, custom, section_border_color, with_section_border_color);
        try_override!(t, custom, section_title_color, with_section_title_color);
        try_override!(t, custom, tool_title_color, with_tool_title_color);
        try_override!(t, custom, tool_value_color, with_tool_value_color);
        try_override!(t, custom, tool_label_color, with_tool_label_color);
        try_override!(t, custom, tool_running_accent, with_tool_running_accent);
        try_override!(t, custom, tool_success_accent, with_tool_success_accent);
        try_override!(t, custom, tool_failed_accent, with_tool_failed_accent);

        // ── Apply plan colour overrides ──
        try_override!(t, custom, plan_progress_color, with_plan_progress_color);
        try_override!(t, custom, plan_summary_color, with_plan_summary_color);
        try_override!(
            t,
            custom,
            plan_explanation_color,
            with_plan_explanation_color
        );
        try_override!(t, custom, plan_pending_color, with_plan_pending_color);
        try_override!(
            t,
            custom,
            plan_in_progress_color,
            with_plan_in_progress_color
        );
        try_override!(t, custom, plan_completed_color, with_plan_completed_color);

        // ── Apply work panel symbol overrides ──
        if let Some(ref s) = custom.work_pending_symbol {
            t.work_pending_symbol = Box::leak(s.clone().into_boxed_str());
        }
        if let Some(ref s) = custom.work_in_progress_symbol {
            t.work_in_progress_symbol = Box::leak(s.clone().into_boxed_str());
        }
        if let Some(ref s) = custom.work_completed_symbol {
            t.work_completed_symbol = Box::leak(s.clone().into_boxed_str());
        }
        if let Some(ref s) = custom.work_failed_symbol {
            t.work_failed_symbol = Box::leak(s.clone().into_boxed_str());
        }
        if let Some(ref s) = custom.work_canceled_symbol {
            t.work_canceled_symbol = Box::leak(s.clone().into_boxed_str());
        }

        t
    }

    /// Resolve a theme setting string that may be a built-in name or
    /// a `file:<name>` reference to a custom theme file in the themes
    /// directory.
    #[must_use]
    pub fn from_setting_or_file(theme_setting: &str) -> Option<Self> {
        if let Some(file_name) = theme_setting.strip_prefix("file:") {
            return Self::load_custom_theme(file_name);
        }
        // Try built-in
        let base = Self::from_setting(theme_setting);
        if base.is_some() {
            return base;
        }
        // Fallback: try loading as a custom theme file name
        Self::load_custom_theme(theme_setting)
    }

    /// Load a custom theme by file name (without directory path).
    /// Searches `~/.config/deepseek/themes/<name>.toml`.
    #[must_use]
    pub fn load_custom_theme(name: &str) -> Option<Self> {
        let path = themes_dir().join(format!("{name}.toml"));
        if path.exists() {
            Self::from_toml_file(&path).ok()
        } else {
            // also try bare name without extension
            let path = themes_dir().join(name);
            if path.exists() {
                Self::from_toml_file(&path).ok()
            } else {
                None
            }
        }
    }

    // ── Tool/plan style helpers (ex deepseek_theme::Theme) ─────────────────

    #[must_use]
    pub const fn tool_status_color(self, status: ToolStatus) -> Color {
        match status {
            ToolStatus::Running => self.tool_running_accent,
            ToolStatus::Success => self.tool_success_accent,
            ToolStatus::Failed => self.tool_failed_accent,
        }
    }

    #[must_use]
    pub fn tool_title_style(self) -> Style {
        Style::default()
            .fg(self.tool_title_color)
            .add_modifier(Modifier::BOLD)
    }

    #[must_use]
    pub fn tool_status_style(self, status: ToolStatus) -> Style {
        Style::default().fg(self.tool_status_color(status))
    }

    #[must_use]
    pub fn tool_label_style(self) -> Style {
        Style::default().fg(self.tool_label_color)
    }

    #[must_use]
    pub fn tool_value_style(self) -> Style {
        Style::default().fg(self.tool_value_color)
    }
}

// ── Backward-compat re-exports from palette ──────────────────────────────────

/// Returns the active theme — kept for `history.rs` tests.
#[must_use]
pub fn active_theme() -> Theme {
    DARK_THEME
}

/// Build a `UiTheme`-equivalent from settings strings (palette.rs compat).
#[must_use]
#[allow(dead_code)] // compat wrapper for external callers
pub fn ui_theme_from_settings(theme: &str, background_color: Option<&str>) -> Theme {
    let mut t = Theme::from_setting(theme).unwrap_or_else(Theme::detect);
    if let Some(background) = background_color.and_then(palette::parse_hex_rgb_color) {
        t = t.with_background_color(background);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette;
    use crate::tui::history::ToolStatus;

    #[test]
    fn active_theme_returns_dark() {
        assert_eq!(active_theme(), DARK_THEME);
    }

    #[test]
    fn dark_theme_matches_existing_palette_choices() {
        let theme = DARK_THEME;
        assert_eq!(theme.mode, PaletteMode::Dark);
        assert_eq!(theme.border_color, palette::BORDER_COLOR);
        assert_eq!(theme.surface_bg, palette::DEEPSEEK_INK);
        assert_eq!(theme.section_title_color, palette::DEEPSEEK_BLUE);
        assert_eq!(theme.tool_title_color, palette::TEXT_SOFT);
        assert_eq!(theme.tool_value_color, palette::TEXT_MUTED);
        assert_eq!(theme.tool_label_color, palette::TEXT_DIM);
        assert_eq!(theme.tool_running_accent, palette::ACCENT_TOOL_LIVE);
        assert_eq!(theme.tool_success_accent, palette::TEXT_DIM);
        assert_eq!(theme.tool_failed_accent, palette::ACCENT_TOOL_ISSUE);
    }

    #[test]
    fn light_theme_uses_light_panel_tokens() {
        let theme = Theme::for_mode(PaletteMode::Light);
        assert_eq!(theme.mode, PaletteMode::Light);
        assert_eq!(theme.panel_bg, palette::LIGHT_PANEL);
        assert_eq!(theme.sidebar_bg, palette::LIGHT_PANEL);
        assert_eq!(theme.border_color, palette::LIGHT_BORDER);
        assert_eq!(theme.tool_title_color, palette::LIGHT_TEXT_SOFT);
        assert_eq!(theme.tool_value_color, palette::LIGHT_TEXT_MUTED);
        assert_eq!(theme.plan_summary_color, palette::LIGHT_TEXT_MUTED);
    }

    #[test]
    fn tool_status_color_maps_each_status() {
        let theme = DARK_THEME;
        assert_eq!(
            theme.tool_status_color(ToolStatus::Running),
            theme.tool_running_accent
        );
        assert_eq!(
            theme.tool_status_color(ToolStatus::Success),
            theme.tool_success_accent
        );
        assert_eq!(
            theme.tool_status_color(ToolStatus::Failed),
            theme.tool_failed_accent
        );
    }

    #[test]
    fn ui_theme_from_settings_applies_theme_and_background() {
        let theme = ui_theme_from_settings("dark", Some("#111111"));
        assert_eq!(theme.mode, PaletteMode::Dark);
        assert_eq!(theme.surface_bg, Color::Rgb(17, 17, 17));
    }

    #[test]
    fn from_settings_with_overrides() {
        let theme = Theme::from_settings("dark", Some("rounded"), None);
        assert_eq!(theme.border_type, BorderType::Rounded);
    }

    #[test]
    fn from_settings_with_section_border_fallback() {
        let theme = Theme::from_settings("dark", Some("rounded"), None);
        assert_eq!(theme.border_type, BorderType::Rounded);
        assert_eq!(theme.section_border_type, BorderType::Rounded);

        let theme2 = Theme::from_settings("dark", Some("rounded"), Some("plain"));
        assert_eq!(theme2.border_type, BorderType::Rounded);
        assert_eq!(theme2.section_border_type, BorderType::Plain);
    }

    #[test]
    fn custom_theme_file_parses_reasoning_bg() {
        let toml_str = "base = \"dark\"\nreasoning_bg = \"#362C1A\"\n";
        let custom: crate::tui::theme::CustomThemeFile =
            toml::from_str(toml_str).expect("parse custom theme");
        assert_eq!(custom.base, "dark");
        assert_eq!(custom.reasoning_bg, Some("#362C1A".to_string()));

        let theme = Theme::from_custom_theme(&custom);
        assert_eq!(theme.reasoning_bg, Some(Color::Rgb(0x36, 0x2C, 0x1A)));
    }

    #[test]
    fn custom_theme_reasoning_bg_reset() {
        let toml_str = "base = \"dark\"\nreasoning_bg = \"reset\"\n";
        let custom: crate::tui::theme::CustomThemeFile =
            toml::from_str(toml_str).expect("parse custom theme");
        let theme = Theme::from_custom_theme(&custom);
        assert_eq!(theme.reasoning_bg, Some(Color::Reset));
    }

    #[test]
    fn border_type_from_setting_roundtrips() {
        assert_eq!(border_type_from_setting("rounded"), BorderType::Rounded);
        assert_eq!(border_type_from_setting("plain"), BorderType::Plain);
        assert_eq!(border_type_from_setting(""), BorderType::Plain);
        assert_eq!(border_type_from_setting("unknown"), BorderType::Plain);
    }

    #[test]
    fn theme_id_from_name_aliases() {
        assert_eq!(ThemeId::from_name("dark"), Some(ThemeId::Whale));
        assert_eq!(ThemeId::from_name("whale"), Some(ThemeId::Whale));
        assert_eq!(ThemeId::from_name("light"), Some(ThemeId::WhaleLight));
        assert_eq!(ThemeId::from_name("system"), Some(ThemeId::System));
        assert_eq!(ThemeId::from_name("nonexistent"), None);
    }
}
