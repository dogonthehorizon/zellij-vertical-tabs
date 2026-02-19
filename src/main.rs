use std::cmp::{max, min};
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;
use zellij_tile::prelude::*;

// ========== COLOR SYSTEM ==========

/// Color specification supporting default, 256-color, and RGB
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum ColorSpec {
    /// Use terminal default color
    #[default]
    Default,
    /// 256-color palette index (0-255)
    EightBit(u8),
    /// True color RGB
    Rgb(u8, u8, u8),
}

impl ColorSpec {
    /// Generate ANSI escape code for foreground color
    fn to_ansi_fg(self) -> String {
        match self {
            ColorSpec::Default => String::new(),
            ColorSpec::EightBit(n) => format!("\x1b[38;5;{}m", n),
            ColorSpec::Rgb(r, g, b) => format!("\x1b[38;2;{};{};{}m", r, g, b),
        }
    }

    /// Generate ANSI escape code for background color
    fn to_ansi_bg(self) -> String {
        match self {
            ColorSpec::Default => String::new(),
            ColorSpec::EightBit(n) => format!("\x1b[48;5;{}m", n),
            ColorSpec::Rgb(r, g, b) => format!("\x1b[48;2;{};{};{}m", r, g, b),
        }
    }

    fn is_default(self) -> bool {
        matches!(self, ColorSpec::Default)
    }
}

/// Parse a color value from string
/// Supports:
/// - Named colors: "accent", "dim", "red", etc.
/// - 256-color: "238"
/// - Hex RGB: "#444444" or "#444"
/// - RGB function: "rgb(68,68,68)"
fn parse_color_spec(name: &str) -> ColorSpec {
    let name = name.trim();

    // Check for RGB hex: #RGB or #RRGGBB
    if let Some(hex) = name.strip_prefix('#')
        && let Some((r, g, b)) = parse_hex_color(hex)
    {
        return ColorSpec::Rgb(r, g, b);
    }

    // Check for rgb(r,g,b) syntax
    if let Some(inner) = name.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')'))
        && let Some((r, g, b)) = parse_rgb_func(inner)
    {
        return ColorSpec::Rgb(r, g, b);
    }

    // Check for numeric 256-color
    if let Ok(n) = name.parse::<u8>() {
        return ColorSpec::EightBit(n);
    }

    // Named colors mapped to 256-color approximations
    match name.to_lowercase().as_str() {
        // Default/reset
        "none" | "default" | "reset" => ColorSpec::Default,

        // Theme-like semantic colors (mapped to reasonable 256-color values)
        "accent" | "primary" => ColorSpec::EightBit(39), // Bright blue
        "secondary" => ColorSpec::EightBit(75),          // Light blue
        "tertiary" => ColorSpec::EightBit(141),          // Purple
        "muted" | "quaternary" => ColorSpec::EightBit(245), // Light gray
        "dim" | "dimmed" => ColorSpec::EightBit(240),    // Dark gray

        // Standard colors
        "black" => ColorSpec::EightBit(0),
        "red" | "error" | "warning" => ColorSpec::EightBit(196),
        "green" | "success" | "ok" => ColorSpec::EightBit(82),
        "yellow" => ColorSpec::EightBit(226),
        "blue" => ColorSpec::EightBit(33),
        "magenta" => ColorSpec::EightBit(201),
        "cyan" => ColorSpec::EightBit(51),
        "white" => ColorSpec::EightBit(15),
        "orange" => ColorSpec::EightBit(208),
        "gray" | "grey" => ColorSpec::EightBit(244),
        "pink" => ColorSpec::EightBit(213),
        "purple" => ColorSpec::EightBit(135),

        // Unknown - use default
        _ => ColorSpec::Default,
    }
}

/// Parse hex color: "444444" or "444" -> (r, g, b)
fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    match hex.len() {
        3 => {
            // #RGB -> expand to #RRGGBB
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some((r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

/// Parse "r,g,b" -> (r, g, b)
fn parse_rgb_func(inner: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].trim().parse::<u8>().ok()?;
    let g = parts[1].trim().parse::<u8>().ok()?;
    let b = parts[2].trim().parse::<u8>().ok()?;
    Some((r, g, b))
}

// ========== STYLE SYSTEM ==========

/// Inline style from #[...] directive
#[derive(Debug, Clone, Default)]
struct InlineStyle {
    fg: ColorSpec,
    bg: ColorSpec,
    bold: bool,
    dim: bool,
    fill: bool,
}

impl InlineStyle {
    /// Generate ANSI escape codes for this style (without reverse - that's handled at line level)
    fn to_ansi(&self) -> String {
        let mut result = String::new();

        // Attributes
        if self.bold {
            result.push_str("\x1b[1m");
        }
        if self.dim {
            result.push_str("\x1b[2m");
        }

        // Colors
        result.push_str(&self.fg.to_ansi_fg());
        result.push_str(&self.bg.to_ansi_bg());

        result
    }

    fn has_any_style(&self) -> bool {
        !self.fg.is_default() || !self.bg.is_default() || self.bold || self.dim || self.fill
    }
}

/// A segment of text with styling
#[derive(Debug, Clone)]
struct StyledSegment {
    text: String,
    style: InlineStyle,
}

impl StyledSegment {
    fn display_width(&self) -> usize {
        self.text.width()
    }
}

/// Collection of styled segments forming a complete styled string
#[derive(Debug, Clone, Default)]
struct StyledText {
    segments: Vec<StyledSegment>,
}

impl StyledText {
    fn new() -> Self {
        Self { segments: vec![] }
    }

    fn push(&mut self, text: String, style: InlineStyle) {
        if !text.is_empty() {
            self.segments.push(StyledSegment { text, style });
        }
    }

    fn display_width(&self) -> usize {
        self.segments.iter().map(|s| s.display_width()).sum()
    }

    /// Render to ANSI-coded string
    fn to_ansi(&self) -> String {
        let mut result = String::new();

        for segment in &self.segments {
            if segment.style.has_any_style() {
                result.push_str("\x1b[0m"); // Reset before applying new style
                result.push_str(&segment.style.to_ansi());
            }
            result.push_str(&segment.text);
        }

        // Reset at end
        if self.segments.iter().any(|s| s.style.has_any_style()) {
            result.push_str("\x1b[0m");
        }

        result
    }

    /// Truncate to fit within max_width display columns
    fn truncate(&self, max_width: usize) -> StyledText {
        if self.display_width() <= max_width {
            return self.clone();
        }

        let mut result = StyledText::new();
        let mut remaining = max_width;

        for segment in &self.segments {
            if remaining == 0 {
                break;
            }

            let seg_width = segment.display_width();
            if seg_width <= remaining {
                result.push(segment.text.clone(), segment.style.clone());
                remaining -= seg_width;
            } else {
                // Truncate this segment
                let mut truncated = String::new();
                let mut width = 0;
                for ch in segment.text.chars() {
                    let ch_width = ch.to_string().width();
                    if width + ch_width > remaining {
                        break;
                    }
                    truncated.push(ch);
                    width += ch_width;
                }
                result.push(truncated, segment.style.clone());
                break;
            }
        }

        result
    }
}

// ========== FORMAT PARSING ==========

/// Token from parsing a tmux-style format string
#[derive(Debug, Clone)]
enum FormatToken {
    /// Style directive: #[fg=color,bg=color,bold,dim]
    Style(InlineStyle),
    /// Variable with optional width: {var} or {=12:var}
    Variable { name: String, width: Option<usize> },
    /// Plain text
    Literal(String),
}

/// Parse a tmux-style format string into tokens
/// Supports: #[fg=color,bg=color,bold,dim], {variable}, {=width:variable}, #{variable}
fn parse_tmux_format(format: &str) -> Vec<FormatToken> {
    let mut tokens = Vec::new();
    let mut chars = format.chars().peekable();
    let mut literal = String::new();

    while let Some(ch) = chars.next() {
        if ch == '#' {
            match chars.peek() {
                Some('[') => {
                    // Flush literal
                    if !literal.is_empty() {
                        tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
                    }
                    chars.next(); // consume '['
                                  // Parse style directive until ']'
                    let mut style_str = String::new();
                    while let Some(&c) = chars.peek() {
                        if c == ']' {
                            chars.next();
                            break;
                        }
                        style_str.push(chars.next().unwrap());
                    }
                    tokens.push(FormatToken::Style(parse_style_directive(&style_str)));
                }
                Some('{') => {
                    // Flush literal
                    if !literal.is_empty() {
                        tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
                    }
                    chars.next(); // consume '{'
                    let var_token = parse_variable(&mut chars);
                    tokens.push(var_token);
                }
                _ => {
                    literal.push(ch);
                }
            }
        } else if ch == '{' {
            // Flush literal
            if !literal.is_empty() {
                tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
            }
            let var_token = parse_variable(&mut chars);
            tokens.push(var_token);
        } else {
            literal.push(ch);
        }
    }

    if !literal.is_empty() {
        tokens.push(FormatToken::Literal(literal));
    }

    tokens
}

/// Parse style directive content: "fg=color,bg=color,bold,dim"
fn parse_style_directive(content: &str) -> InlineStyle {
    let mut style = InlineStyle::default();

    for part in content.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some(color_str) = part.strip_prefix("fg=") {
            style.fg = parse_color_spec(color_str);
        } else if let Some(color_str) = part.strip_prefix("bg=") {
            style.bg = parse_color_spec(color_str);
        } else if part == "bold" {
            style.bold = true;
        } else if part == "dim" {
            style.dim = true;
        } else if part == "fill" {
            style.fill = true;
        } else if part == "default" || part == "none" || part == "reset" {
            style = InlineStyle::default();
        }
    }

    style
}

/// Parse variable content after '{': "var}" or "=12:var}"
fn parse_variable(chars: &mut std::iter::Peekable<std::str::Chars>) -> FormatToken {
    let mut content = String::new();
    while let Some(&c) = chars.peek() {
        if c == '}' {
            chars.next();
            break;
        }
        content.push(chars.next().unwrap());
    }

    // Check for width specifier: =12:varname
    if let Some(rest) = content.strip_prefix('=')
        && let Some(colon_pos) = rest.find(':')
    {
        let width_str = &rest[..colon_pos];
        let var_name = &rest[colon_pos + 1..];
        if let Ok(width) = width_str.parse::<usize>() {
            return FormatToken::Variable {
                name: var_name.to_string(),
                width: Some(width),
            };
        }
    }

    FormatToken::Variable {
        name: content,
        width: None,
    }
}

/// Parse a styled string like "#[fg=240]│" into StyledText
fn parse_styled_string(s: &str) -> StyledText {
    let tokens = parse_tmux_format(s);
    let mut result = StyledText::new();
    let mut current_style = InlineStyle::default();

    for token in tokens {
        match token {
            FormatToken::Style(style) => {
                current_style = style;
            }
            FormatToken::Literal(text) => {
                result.push(text, current_style.clone());
            }
            FormatToken::Variable { name, .. } => {
                // Variables in border strings are not expanded, treat as literal
                result.push(format!("{{{}}}", name), current_style.clone());
            }
        }
    }

    result
}

// ========== COMMAND SYSTEM ==========

/// A single shell command with its polling interval
#[derive(Debug, Clone)]
struct CommandDef {
    /// Full shell command string (run via bash -c)
    command: String,
    /// Seconds between executions
    interval: f64,
    /// Seconds elapsed since last execution (pre-filled to interval to fire immediately)
    elapsed: f64,
}

// ========== CONFIGURATION ==========

/// Rendering tier based on available column width
#[derive(Debug, Clone, Copy, PartialEq)]
enum RenderTier {
    Full,
    Compact,
    Minimal,
}

/// Full styling configuration for the sidebar
#[derive(Clone)]
struct StyleConfig {
    // ---- Tab formatting ----
    format: String,
    format_active: String,
    overflow_above: String,
    overflow_below: String,
    indicator_active: String,
    indicator_fullscreen: String,
    indicator_sync: String,
    padding_top: usize,
    border: String,
    max_name_length: usize,
    start_index: usize,

    // ---- Header ----
    /// Newline-separated format string for header area. Supports {session}, {mode}.
    format_header: String,
    /// Styled separator rendered below the header (e.g. "#[fg=#494d64]─"). Empty = none.
    header_border: String,
    /// Per-mode display strings: mode_key -> (full, compact, minimal)
    mode_labels: BTreeMap<String, (String, String, String)>,
    /// Per-mode foreground color string (hex/256): mode_key -> color
    mode_colors: BTreeMap<String, String>,

    // ---- Footer ----
    /// Newline-separated format string for footer area. Supports {command_NAME}.
    format_footer: String,
    format_footer_compact: String,
    format_footer_minimal: String,
    /// Styled separator rendered above the footer. Empty = none.
    footer_border: String,

    // ---- Adaptive thresholds ----
    compact_threshold: usize,
    minimal_threshold: usize,
}

impl Default for StyleConfig {
    fn default() -> Self {
        let mut mode_labels: BTreeMap<String, (String, String, String)> = BTreeMap::new();
        for (key, full, short, icon) in &[
            ("normal", "NORMAL", "NRM", "N"),
            ("locked", "LOCKED", "LCK", "L"),
            ("resize", "RESIZE", "RSZ", "R"),
            ("pane", "PANE", "PNE", "P"),
            ("tab", "TAB", "TAB", "T"),
            ("scroll", "SCROLL", "SCR", "S"),
            ("entersearch", "SEARCH", "SRC", "?"),
            ("search", "SEARCH", "SRC", "?"),
            ("renametab", "RENAME", "RNM", "R"),
            ("renamepane", "RENAME", "RNM", "R"),
            ("session", "SESSION", "SES", "S"),
            ("move", "MOVE", "MOV", "M"),
            ("prompt", "PROMPT", "PRM", "P"),
            ("tmux", "TMUX", "TMX", "T"),
        ] {
            mode_labels.insert(
                key.to_string(),
                (full.to_string(), short.to_string(), icon.to_string()),
            );
        }

        Self {
            format: "{index}:{name}".to_string(),
            format_active: "{index}:{name} {indicators}".to_string(),
            overflow_above: "  ^ +{count}".to_string(),
            overflow_below: "  v +{count}".to_string(),
            indicator_active: "*".to_string(),
            indicator_fullscreen: "Z".to_string(),
            indicator_sync: "S".to_string(),
            max_name_length: 20,
            padding_top: 0,
            border: String::new(),
            start_index: 1,
            format_header: "{session}  {mode}".to_string(),
            header_border: String::new(),
            mode_labels,
            mode_colors: BTreeMap::new(),
            format_footer: String::new(),
            format_footer_compact: String::new(),
            format_footer_minimal: String::new(),
            footer_border: String::new(),
            compact_threshold: 18,
            minimal_threshold: 12,
        }
    }
}

impl StyleConfig {
    fn render_tier(&self, cols: usize) -> RenderTier {
        if cols < self.minimal_threshold {
            RenderTier::Minimal
        } else if cols < self.compact_threshold {
            RenderTier::Compact
        } else {
            RenderTier::Full
        }
    }

    fn active_footer_format(&self, tier: RenderTier) -> &str {
        match tier {
            RenderTier::Minimal if !self.format_footer_minimal.is_empty() => {
                &self.format_footer_minimal
            }
            RenderTier::Compact if !self.format_footer_compact.is_empty() => {
                &self.format_footer_compact
            }
            _ => &self.format_footer,
        }
    }

    /// Returns (label_string, optional_color_string) for the current mode at the given tier.
    fn mode_display(&self, mode: &InputMode, tier: RenderTier) -> (String, Option<String>) {
        let key = input_mode_key(mode);
        let label = self
            .mode_labels
            .get(key)
            .map(|(full, compact, minimal)| match tier {
                RenderTier::Full => full.clone(),
                RenderTier::Compact => compact.clone(),
                RenderTier::Minimal => minimal.clone(),
            })
            .unwrap_or_else(|| format!("{:?}", mode).to_uppercase());
        let color = self.mode_colors.get(key).cloned();
        (label, color)
    }
}

fn input_mode_key(mode: &InputMode) -> &'static str {
    match mode {
        InputMode::Normal => "normal",
        InputMode::Locked => "locked",
        InputMode::Resize => "resize",
        InputMode::Pane => "pane",
        InputMode::Tab => "tab",
        InputMode::Scroll => "scroll",
        InputMode::EnterSearch => "entersearch",
        InputMode::Search => "search",
        InputMode::RenameTab => "renametab",
        InputMode::RenamePane => "renamepane",
        InputMode::Session => "session",
        InputMode::Move => "move",
        InputMode::Prompt => "prompt",
        InputMode::Tmux => "tmux",
    }
}

// ========== PLUGIN STATE ==========

#[derive(Default)]
struct State {
    tabs: Vec<TabInfo>,
    active_tab_idx: usize,
    mode_info: ModeInfo,
    pane_manifest: PaneManifest,
    style: StyleConfig,
    last_rows: usize,
    permissions_granted: bool,
    is_selectable: bool,
    pending_events: Vec<Event>,

    // Command execution system
    commands: BTreeMap<String, CommandDef>,
    command_outputs: BTreeMap<String, String>,
    timer_ticking: bool,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        // ---- Tab formatting ----
        if let Some(v) = configuration.get("format") {
            self.style.format = v.clone();
        }
        if let Some(v) = configuration.get("format_active") {
            self.style.format_active = v.clone();
        }
        if let Some(v) = configuration.get("overflow_above") {
            self.style.overflow_above = v.clone();
        }
        if let Some(v) = configuration.get("overflow_below") {
            self.style.overflow_below = v.clone();
        }
        if let Some(v) = configuration.get("indicator_active") {
            self.style.indicator_active = v.clone();
        }
        if let Some(v) = configuration.get("indicator_fullscreen") {
            self.style.indicator_fullscreen = v.clone();
        }
        if let Some(v) = configuration.get("indicator_sync") {
            self.style.indicator_sync = v.clone();
        }
        if let Some(v) = configuration.get("max_name_length")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.max_name_length = n;
        }
        if let Some(v) = configuration.get("padding_top")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.padding_top = n;
        }
        if let Some(v) = configuration.get("border") {
            self.style.border = v.clone();
        } else if let Some(v) = configuration.get("border_char") {
            self.style.border = v.clone();
        }
        if let Some(v) = configuration.get("start_index")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.start_index = n;
        }

        // ---- Header ----
        if let Some(v) = configuration.get("format_header") {
            self.style.format_header = v.clone();
        }
        if let Some(v) = configuration.get("header_border") {
            self.style.header_border = v.clone();
        }

        // Mode labels: mode_NAME / mode_NAME_compact / mode_NAME_minimal
        // Mode colors: mode_color_NAME
        for mode_key in &[
            "normal",
            "locked",
            "resize",
            "pane",
            "tab",
            "scroll",
            "entersearch",
            "search",
            "renametab",
            "renamepane",
            "session",
            "move",
            "prompt",
            "tmux",
        ] {
            let defaults = self
                .style
                .mode_labels
                .get(*mode_key)
                .cloned()
                .unwrap_or_else(|| {
                    (
                        mode_key.to_uppercase(),
                        mode_key.to_uppercase(),
                        mode_key[..1].to_uppercase(),
                    )
                });

            let full = configuration
                .get(&format!("mode_{}", mode_key))
                .cloned()
                .unwrap_or(defaults.0);
            let compact = configuration
                .get(&format!("mode_{}_compact", mode_key))
                .cloned()
                .unwrap_or(defaults.1);
            let minimal = configuration
                .get(&format!("mode_{}_minimal", mode_key))
                .cloned()
                .unwrap_or(defaults.2);

            self.style
                .mode_labels
                .insert(mode_key.to_string(), (full, compact, minimal));

            if let Some(color) = configuration.get(&format!("mode_color_{}", mode_key)) {
                self.style
                    .mode_colors
                    .insert(mode_key.to_string(), color.clone());
            }
        }

        // ---- Footer ----
        if let Some(v) = configuration.get("format_footer") {
            self.style.format_footer = v.clone();
        }
        if let Some(v) = configuration.get("format_footer_compact") {
            self.style.format_footer_compact = v.clone();
        }
        if let Some(v) = configuration.get("format_footer_minimal") {
            self.style.format_footer_minimal = v.clone();
        }
        if let Some(v) = configuration.get("footer_border") {
            self.style.footer_border = v.clone();
        }

        // ---- Adaptive thresholds ----
        if let Some(v) = configuration.get("compact_threshold")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.compact_threshold = n;
        }
        if let Some(v) = configuration.get("minimal_threshold")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.minimal_threshold = n;
        }

        // ---- Commands ----
        // Parse command_NAME_command / command_NAME_interval pairs
        let mut command_names: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for key in configuration.keys() {
            if let Some(rest) = key.strip_prefix("command_")
                && let Some(name) = rest.strip_suffix("_command")
            {
                command_names.insert(name.to_string());
            }
        }

        for name in command_names {
            let cmd_key = format!("command_{}_command", name);
            let interval_key = format!("command_{}_interval", name);

            if let Some(command) = configuration.get(&cmd_key) {
                let interval = configuration
                    .get(&interval_key)
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(30.0);

                self.commands.insert(
                    name,
                    CommandDef {
                        command: command.clone(),
                        interval,
                        // Pre-fill elapsed so the first timer tick fires immediately
                        elapsed: interval,
                    },
                );
            }
        }

        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
        ]);

        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ModeUpdate,
            EventType::Mouse,
            EventType::PermissionRequestResult,
            EventType::RunCommandResult,
            EventType::Timer,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        let mut should_render = false;

        if let Event::PermissionRequestResult(status) = event {
            if status == PermissionStatus::Granted {
                self.permissions_granted = true;
                self.is_selectable = false;
                set_selectable(false);

                // Start the timer loop if commands are configured
                if !self.commands.is_empty() && !self.timer_ticking {
                    self.timer_ticking = true;
                    set_timeout(1.0);
                }

                while !self.pending_events.is_empty() {
                    let cached_event = self.pending_events.remove(0);
                    self.update(cached_event);
                }
                should_render = true;
            }
            return should_render;
        }

        if !self.permissions_granted {
            self.pending_events.push(event);
            return false;
        }

        match event {
            Event::PermissionRequestResult(_) => {}
            Event::ModeUpdate(mode_info) => {
                if self.mode_info != mode_info {
                    should_render = true;
                }
                self.mode_info = mode_info;
            }
            Event::TabUpdate(tabs) => {
                let active_tab_index = tabs.iter().position(|t| t.active).unwrap_or(0);
                let active_tab_idx = active_tab_index + 1;
                if self.active_tab_idx != active_tab_idx || self.tabs != tabs {
                    should_render = true;
                }
                self.active_tab_idx = active_tab_idx;
                self.tabs = tabs;
            }
            Event::PaneUpdate(pane_manifest) => {
                self.pane_manifest = pane_manifest;
                should_render = true;
            }
            Event::Mouse(me) => match me {
                Mouse::LeftClick(row, _col) => {
                    if let Some(idx) = self.get_tab_at_row(row as usize) {
                        switch_tab_to(idx as u32);
                    }
                }
                Mouse::ScrollUp(_) => {
                    let prev_tab = max(self.active_tab_idx.saturating_sub(1), 1);
                    switch_tab_to(prev_tab as u32);
                }
                Mouse::ScrollDown(_) => {
                    let next_tab = min(self.active_tab_idx + 1, self.tabs.len());
                    switch_tab_to(next_tab as u32);
                }
                _ => {}
            },
            Event::Timer(_) => {
                // Tick each command's elapsed counter; fire those that are due
                let due: Vec<(String, String)> = self
                    .commands
                    .iter_mut()
                    .filter_map(|(name, def)| {
                        def.elapsed += 1.0;
                        if def.elapsed >= def.interval {
                            def.elapsed = 0.0;
                            Some((name.clone(), def.command.clone()))
                        } else {
                            None
                        }
                    })
                    .collect();

                for (name, command) in due {
                    let mut context = BTreeMap::new();
                    context.insert("command_name".to_string(), name);
                    run_command(&["bash", "-c", &command], context);
                }

                // Re-arm the 1-second tick
                if self.timer_ticking {
                    set_timeout(1.0);
                }
            }
            Event::RunCommandResult(exit_code, stdout, _stderr, context) => {
                if let Some(name) = context.get("command_name") {
                    if exit_code == Some(0) || exit_code.is_none() {
                        let output = String::from_utf8_lossy(&stdout).trim().to_string();
                        self.command_outputs.insert(name.clone(), output);
                    } else {
                        // On error, keep prior value or insert "--"
                        self.command_outputs
                            .entry(name.clone())
                            .or_insert_with(|| "--".to_string());
                    }
                    should_render = true;
                }
            }
            _ => {}
        }
        should_render
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        match pipe_message.name.as_str() {
            "set_selectable" => {
                match pipe_message.payload.as_deref() {
                    Some("true") => {
                        self.is_selectable = true;
                        set_selectable(true);
                    }
                    Some("false") => {
                        self.is_selectable = false;
                        set_selectable(false);
                    }
                    _ => {}
                }
                false
            }
            "toggle_selectable" => {
                self.is_selectable = !self.is_selectable;
                set_selectable(self.is_selectable);
                false
            }
            _ => false,
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        self.last_rows = rows;

        if !self.permissions_granted || self.tabs.is_empty() {
            return;
        }

        self.render_sidebar(rows, cols);
    }
}

impl State {
    fn get_focused_pane_title(&self, tab_position: usize) -> Option<String> {
        if let Some(panes) = self.pane_manifest.panes.get(&tab_position) {
            for pane in panes {
                if pane.is_focused && !pane.is_plugin {
                    let title = &pane.title;
                    if title.starts_with("Pane #") || title.starts_with("Tab #") || title.is_empty()
                    {
                        return None;
                    }
                    return Some(title.clone());
                }
            }
        }
        None
    }

    fn expand_overflow_format(&self, format: &str, count: usize) -> String {
        format.replace("{count}", &count.to_string())
    }

    /// Expand a generic format string (header/footer) with all available variables.
    fn expand_generic_format(&self, format: &str, tier: RenderTier) -> StyledText {
        let tokens = parse_tmux_format(format);
        let mut result = StyledText::new();
        let mut current_style = InlineStyle::default();

        let session_name = self
            .mode_info
            .session_name
            .clone()
            .unwrap_or_else(|| "zellij".to_string());

        let (mode_label, mode_color_str) = self.style.mode_display(&self.mode_info.mode, tier);

        for token in tokens {
            match token {
                FormatToken::Style(style) => {
                    current_style = style;
                }
                FormatToken::Literal(text) => {
                    result.push(text, current_style.clone());
                }
                FormatToken::Variable { name, width } => {
                    match name.as_str() {
                        "session" => {
                            let text = if let Some(w) = width {
                                truncate_string(&session_name, w)
                            } else {
                                session_name.clone()
                            };
                            result.push(text, current_style.clone());
                        }
                        "mode" => {
                            // Apply mode-specific color override if configured
                            let mut mode_style = current_style.clone();
                            if let Some(ref color_str) = mode_color_str {
                                mode_style.fg = parse_color_spec(color_str);
                            }
                            let text = if let Some(w) = width {
                                truncate_string(&mode_label, w)
                            } else {
                                mode_label.clone()
                            };
                            result.push(text, mode_style);
                        }
                        other => {
                            // command_NAME variables
                            let raw = if let Some(cmd_name) = other.strip_prefix("command_") {
                                self.command_outputs
                                    .get(cmd_name)
                                    .cloned()
                                    .unwrap_or_else(|| "--".to_string())
                            } else {
                                format!("{{{}}}", other)
                            };
                            let text = if let Some(w) = width {
                                truncate_string(&raw, w)
                            } else {
                                raw
                            };
                            result.push(text, current_style.clone());
                        }
                    }
                }
            }
        }

        result
    }

    /// Expand a tmux-style format string with tab info, returning styled text
    fn expand_tmux_format(&self, format: &str, tab: &TabInfo, index: usize) -> StyledText {
        let tokens = parse_tmux_format(format);
        let mut result = StyledText::new();
        let mut current_style = InlineStyle::default();

        // Get focused pane title for this tab
        let pane_title = self
            .get_focused_pane_title(tab.position)
            .or_else(|| {
                if !tab.name.starts_with("Tab #") {
                    Some(tab.name.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "...".to_string());

        // Build indicators string
        let mut indicators = String::new();
        if tab.is_fullscreen_active {
            indicators.push_str(&self.style.indicator_fullscreen);
        }
        if tab.is_sync_panes_active {
            indicators.push_str(&self.style.indicator_sync);
        }
        if tab.active {
            indicators.push_str(&self.style.indicator_active);
        }

        for token in tokens {
            match token {
                FormatToken::Style(style) => {
                    current_style = style;
                }
                FormatToken::Variable { name, width } => {
                    let value = match name.as_str() {
                        "index" | "i" => index.to_string(),
                        "name" | "n" => {
                            if tab.active
                                && self.mode_info.mode == InputMode::RenameTab
                                && tab.name.is_empty()
                            {
                                "Enter name...".to_string()
                            } else if !tab.name.starts_with("Tab #") && !tab.name.is_empty() {
                                tab.name.clone()
                            } else {
                                pane_title.clone()
                            }
                        }
                        "title" | "t" | "pane_title" => pane_title.clone(),
                        "indicators" => indicators.clone(),
                        "fullscreen" => {
                            if tab.is_fullscreen_active {
                                self.style.indicator_fullscreen.clone()
                            } else {
                                String::new()
                            }
                        }
                        "sync" => {
                            if tab.is_sync_panes_active {
                                self.style.indicator_sync.clone()
                            } else {
                                String::new()
                            }
                        }
                        "active" => {
                            if tab.active {
                                self.style.indicator_active.clone()
                            } else {
                                String::new()
                            }
                        }
                        _ => format!("{{{}}}", name),
                    };

                    let text = if let Some(w) = width {
                        truncate_string(&value, w)
                    } else {
                        truncate_string(&value, self.style.max_name_length)
                    };

                    result.push(text, current_style.clone());
                }
                FormatToken::Literal(text) => {
                    result.push(text, current_style.clone());
                }
            }
        }

        result
    }

    /// Build a complete tab line with content, optional fill, padding, and side border
    fn build_line(&self, content: &StyledText, cols: usize, is_selected: bool) -> String {
        let border = parse_styled_string(&self.style.border);
        let border_width = border.display_width();

        let effective_cols = cols.saturating_sub(border_width);

        // Truncate content if it exceeds available width to prevent wrapping
        let content = content.truncate(effective_cols);
        let content_width = content.display_width();
        let padding_needed = effective_cols.saturating_sub(content_width);

        let mut line = String::new();

        // Check if any segment has fill attribute - fills entire row with bg color
        let has_fill = is_selected && content.segments.iter().any(|s| s.style.fill);

        if has_fill {
            // Fill mode: use reverse video with swapped colors so bg fills the row
            line.push_str("\x1b[7m");

            for segment in &content.segments {
                let mut swapped_style = segment.style.clone();
                std::mem::swap(&mut swapped_style.fg, &mut swapped_style.bg);
                swapped_style.fill = false;

                if swapped_style.has_any_style() {
                    line.push_str("\x1b[0m\x1b[7m");
                    line.push_str(&swapped_style.to_ansi());
                }
                line.push_str(&segment.text);
            }

            if padding_needed > 0 {
                line.push_str(&" ".repeat(padding_needed));
            }

            line.push_str("\x1b[0m");
        } else {
            line.push_str(&content.to_ansi());

            if padding_needed > 0 {
                line.push_str(&" ".repeat(padding_needed));
            }
        }

        // Add right-side border (not affected by selection fill)
        if border_width > 0 {
            line.push_str(&border.to_ansi());
        }

        line
    }

    /// Build a plain line (no tab side-border): truncated content + space padding
    fn build_plain_line(&self, content: &StyledText, cols: usize) -> String {
        let content = content.truncate(cols);
        let content_width = content.display_width();
        let padding_needed = cols.saturating_sub(content_width);
        let mut line = content.to_ansi();
        if padding_needed > 0 {
            line.push_str(&" ".repeat(padding_needed));
        }
        line
    }

    /// Build a full-width separator line by repeating the first char of the separator format
    fn build_separator_line(&self, separator_format: &str, cols: usize) -> String {
        if separator_format.is_empty() || cols == 0 {
            return " ".repeat(cols);
        }

        let styled = parse_styled_string(separator_format);
        if styled.segments.is_empty() {
            return " ".repeat(cols);
        }

        let fill_char = styled.segments[0].text.chars().next().unwrap_or('─');
        let fill_char_width = fill_char.to_string().width().max(1);
        let repeat_count = cols / fill_char_width;
        let remainder = cols % fill_char_width;

        let mut filled = StyledText::new();
        filled.push(
            fill_char.to_string().repeat(repeat_count),
            styled.segments[0].style.clone(),
        );

        let mut line = filled.to_ansi();
        if remainder > 0 {
            line.push_str(&" ".repeat(remainder));
        }
        line
    }

    /// Build an empty line (spaces + optional side border)
    fn build_empty_line(&self, cols: usize) -> String {
        let border = parse_styled_string(&self.style.border);
        let border_width = border.display_width();

        if border_width == 0 {
            return " ".repeat(cols);
        }

        let effective_cols = cols.saturating_sub(border_width);
        let mut line = " ".repeat(effective_cols);
        line.push_str(&border.to_ansi());
        line
    }

    /// Main render function: computes row budgets then renders header → tabs → footer.
    fn render_sidebar(&mut self, rows: usize, cols: usize) {
        let tier = self.style.render_tier(cols);

        // ---- Compute row budgets ----
        let header_content_rows = if self.style.format_header.is_empty() {
            0
        } else {
            self.style.format_header.lines().count().max(1)
        };
        let header_border_rows =
            usize::from(!self.style.header_border.is_empty() && header_content_rows > 0);
        let header_rows = header_content_rows + header_border_rows;

        let footer_format = self.style.active_footer_format(tier).to_string();
        let footer_content_rows = if footer_format.is_empty() {
            0
        } else {
            footer_format.lines().count().max(1)
        };
        let footer_border_rows =
            usize::from(!self.style.footer_border.is_empty() && footer_content_rows > 0);
        let footer_rows = footer_content_rows + footer_border_rows;

        let top_padding = self.style.padding_top;

        let available_for_tabs = rows
            .saturating_sub(header_rows)
            .saturating_sub(footer_rows)
            .saturating_sub(top_padding);

        // ---- Assemble lines ----
        let mut lines: Vec<String> = Vec::with_capacity(rows);

        // Top padding
        for _ in 0..top_padding {
            lines.push(self.build_empty_line(cols));
        }

        // Header content (each newline-separated line)
        if header_content_rows > 0 {
            for header_line in self.style.format_header.clone().lines() {
                let styled = self.expand_generic_format(header_line, tier);
                lines.push(self.build_plain_line(&styled, cols));
            }
        }

        // Header separator
        if header_border_rows > 0 {
            let sep = self.style.header_border.clone();
            lines.push(self.build_separator_line(&sep, cols));
        }

        // Tabs
        let tab_lines = self.render_tab_lines(available_for_tabs, cols);
        lines.extend(tab_lines);

        // Gap fill (pushes footer to the bottom)
        while lines.len() < rows.saturating_sub(footer_rows) {
            lines.push(self.build_empty_line(cols));
        }

        // Footer separator
        if footer_border_rows > 0 {
            let sep = self.style.footer_border.clone();
            lines.push(self.build_separator_line(&sep, cols));
        }

        // Footer content
        if footer_content_rows > 0 {
            for footer_line in footer_format.lines() {
                let styled = self.expand_generic_format(footer_line, tier);
                lines.push(self.build_plain_line(&styled, cols));
            }
        }

        // Safety clamp (should never be needed, but prevents terminal corruption)
        lines.truncate(rows);

        // Print all lines
        for (i, line) in lines.iter().enumerate() {
            if i < lines.len() - 1 {
                println!("{}\x1b[m", line);
            } else {
                print!("{}\x1b[m", line);
            }
        }
    }

    /// Render tab entries into the available row budget, returns Vec of ANSI strings.
    fn render_tab_lines(&self, available_rows: usize, cols: usize) -> Vec<String> {
        if self.tabs.is_empty() || available_rows == 0 {
            return vec![];
        }

        let tab_count = self.tabs.len();
        let active_index = self.active_tab_idx.saturating_sub(1);

        let (start_index, end_index, tabs_above, tabs_below) =
            calculate_visible_range(tab_count, available_rows, active_index);

        let mut lines: Vec<String> = Vec::with_capacity(available_rows);

        // "above" overflow indicator
        if tabs_above > 0 {
            let text = self.expand_overflow_format(&self.style.overflow_above, tabs_above);
            let styled = parse_styled_string(&text);
            lines.push(self.build_line(&styled, cols, false));
        }

        // Visible tab entries
        for i in start_index..end_index {
            if let Some(tab) = self.tabs.get(i).cloned() {
                let is_active = tab.active;
                let format = if is_active {
                    &self.style.format_active.clone()
                } else {
                    &self.style.format.clone()
                };
                let styled = self.expand_tmux_format(format, &tab, i + self.style.start_index);
                lines.push(self.build_line(&styled, cols, is_active));
            }
        }

        // "below" overflow indicator
        if tabs_below > 0 {
            let text = self.expand_overflow_format(&self.style.overflow_below, tabs_below);
            let styled = parse_styled_string(&text);
            lines.push(self.build_line(&styled, cols, false));
        }

        lines
    }

    fn get_tab_at_row(&self, row: usize) -> Option<usize> {
        if self.tabs.is_empty() {
            return None;
        }

        // Compute how many rows the header occupies so we can offset the click
        let header_content_rows = if self.style.format_header.is_empty() {
            0
        } else {
            self.style.format_header.lines().count().max(1)
        };
        let header_border_rows =
            usize::from(!self.style.header_border.is_empty() && header_content_rows > 0);
        let header_rows = header_content_rows + header_border_rows + self.style.padding_top;

        // Clicks in the header area are not tab clicks
        if row < header_rows {
            return None;
        }
        let row_in_tabs = row - header_rows;

        let tab_count = self.tabs.len();
        let active_index = self.active_tab_idx.saturating_sub(1);

        // Estimate footer rows to compute available_for_tabs
        let footer_rows = if self.style.format_footer.is_empty() {
            0
        } else {
            self.style.format_footer.lines().count()
                + usize::from(!self.style.footer_border.is_empty())
        };
        let available_rows = self
            .last_rows
            .saturating_sub(header_rows)
            .saturating_sub(footer_rows);

        let (start_index, end_index, tabs_above, _tabs_below) =
            calculate_visible_range(tab_count, available_rows, active_index);

        let content_start_row = usize::from(tabs_above > 0);

        // Click on "above" overflow indicator -> go to previous tab
        if tabs_above > 0 && row_in_tabs == 0 {
            let target = start_index.saturating_sub(1);
            return Some(target + 1);
        }

        let row_in_content = row_in_tabs.saturating_sub(content_start_row);
        let clicked_tab_index = start_index + row_in_content;

        if clicked_tab_index < end_index && clicked_tab_index < tab_count {
            return Some(clicked_tab_index + 1);
        }

        if row_in_content >= end_index - start_index {
            let target = end_index.min(tab_count.saturating_sub(1));
            return Some(target + 1);
        }

        None
    }
}

fn calculate_visible_range(
    tab_count: usize,
    available_rows: usize,
    active_index: usize,
) -> (usize, usize, usize, usize) {
    if tab_count == 0 {
        return (0, 0, 0, 0);
    }

    if tab_count <= available_rows {
        return (0, tab_count, 0, 0);
    }

    let max_visible = available_rows.saturating_sub(2);
    if max_visible == 0 {
        return (0, 0, tab_count, 0);
    }

    let mut start_index = active_index;
    let mut end_index = active_index + 1;
    let mut room_left = max_visible.saturating_sub(1);
    let mut alternate = false;

    while room_left > 0 {
        if !alternate && start_index > 0 {
            start_index -= 1;
            room_left -= 1;
        } else if alternate && end_index < tab_count {
            end_index += 1;
            room_left -= 1;
        } else if start_index > 0 {
            start_index -= 1;
            room_left -= 1;
        } else if end_index < tab_count {
            end_index += 1;
            room_left -= 1;
        } else {
            break;
        }
        alternate = !alternate;
    }

    (
        start_index,
        end_index,
        start_index,
        tab_count.saturating_sub(end_index),
    )
}

fn truncate_string(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }

    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let mut truncated = String::new();
    let mut width = 0;
    for ch in s.chars() {
        let ch_width = ch.to_string().width();
        if width + ch_width + 3 > max_width {
            truncated.push_str("...");
            break;
        }
        truncated.push(ch);
        width += ch_width;
    }
    truncated
}
