use std::cmp::Ordering;
use std::fmt::Write;
use std::sync::{Arc, Mutex, OnceLock};

pub const INTERNAL_CMD_CLASS: &str = "dbg";

const HISTORY_MAX: usize = 5;
const SIZE_CMD_ID_MAX: usize = 16;
const PROMPT_PREFIX: &str = "core@app:~# ";
const WELCOME_MSG: &str = "\r\n<unknown package>\r\nSystem Terminal\r\n\r\ntype 'help' or just 'h' for a list of available commands\r\n\r\n";

pub type CommandHandler = Arc<dyn Fn(&str, &CommandRegistry) -> String + Send + Sync>;

#[derive(Clone)]
pub struct SystemCommand {
    pub id: String,
    pub shortcut: String,
    pub desc: String,
    pub group: String,
    handler: CommandHandler,
}

#[derive(Clone, Default)]
pub struct CommandRegistry {
    commands: Vec<SystemCommand>,
}

static GLOBAL_COMMANDS: OnceLock<Mutex<CommandRegistry>> = OnceLock::new();
static BUILTINS_INIT: OnceLock<()> = OnceLock::new();

fn global_commands() -> &'static Mutex<CommandRegistry> {
    GLOBAL_COMMANDS.get_or_init(|| Mutex::new(CommandRegistry::new()))
}

fn command_sort(left: &SystemCommand, right: &SystemCommand) -> Ordering {
    if left.group == INTERNAL_CMD_CLASS && right.group != INTERNAL_CMD_CLASS {
        return Ordering::Less;
    }
    if left.group != INTERNAL_CMD_CLASS && right.group == INTERNAL_CMD_CLASS {
        return Ordering::Greater;
    }

    match left.group.cmp(&right.group) {
        Ordering::Equal => {}
        ord => return ord,
    }

    if !left.shortcut.is_empty() && right.shortcut.is_empty() {
        return Ordering::Less;
    }
    if left.shortcut.is_empty() && !right.shortcut.is_empty() {
        return Ordering::Greater;
    }

    left.id.cmp(&right.id)
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        id: impl Into<String>,
        handler: CommandHandler,
        shortcut: impl Into<String>,
        desc: impl Into<String>,
        group: impl Into<String>,
    ) -> bool {
        let command = SystemCommand {
            id: id.into(),
            shortcut: shortcut.into(),
            desc: desc.into(),
            group: group.into(),
            handler,
        };

        if self
            .commands
            .iter()
            .any(|existing| existing.id == command.id && !command.id.is_empty())
        {
            return false;
        }

        if !command.shortcut.is_empty()
            && self
                .commands
                .iter()
                .any(|existing| existing.shortcut == command.shortcut)
        {
            return false;
        }

        self.commands.push(command);
        self.commands.sort_by(command_sort);
        true
    }

    pub fn execute_line(&self, line: &str) -> String {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let id = parts.next().unwrap_or_default();
        let args = parts.next().unwrap_or_default().trim();

        let Some(command) = self.commands.iter().find(|command| {
            command.id == id || (!command.shortcut.is_empty() && command.shortcut == id)
        }) else {
            return String::from("Command not found");
        };

        (command.handler)(args, self)
    }

    pub fn help_text(&self) -> String {
        let mut out = String::from("\nAvailable commands\n");
        let mut group = String::new();

        for command in &self.commands {
            if command.group != group {
                out.push('\n');
                if !command.group.is_empty() && command.group != INTERNAL_CMD_CLASS {
                    out.push_str(&command.group);
                    out.push('\n');
                }
                group = command.group.clone();
            }

            out.push_str("  ");

            if command.shortcut.is_empty() {
                out.push_str("   ");
            } else {
                let _ = write!(out, "{}, ", command.shortcut);
            }

            let _ = write!(out, "{:<width$}", command.id, width = SIZE_CMD_ID_MAX + 2);

            if !command.desc.is_empty() {
                out.push_str(".. ");
                out.push_str(&command.desc);
            }

            out.push('\n');
        }

        out.trim_end().to_owned()
    }

    pub fn candidates(&self, prefix: &str) -> Vec<&str> {
        self.commands
            .iter()
            .filter_map(|command| {
                command
                    .id
                    .starts_with(prefix)
                    .then_some(command.id.as_str())
            })
            .collect()
    }
}

fn register_builtin(id: &str, handler: CommandHandler, shortcut: &str, desc: &str, group: &str) {
    let _ = global_commands()
        .lock()
        .unwrap()
        .register(id, handler, shortcut, desc, group);
}

fn parse_number(value: &str) -> Option<usize> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        usize::from_str_radix(hex, 16).ok()
    } else {
        value.parse::<usize>().ok()
    }
}

fn hex_dump_command(args: &str, _registry: &CommandRegistry) -> String {
    let mut parts = args.split_whitespace();
    let Some(address) = parts.next().and_then(parse_number) else {
        return String::from("Specify address");
    };

    let len = parts.next().and_then(parse_number).unwrap_or(16);
    if len == 0 {
        return String::from("Length must be greater than zero");
    }

    let ptr = address as *const u8;
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    hex_dump_bytes(ptr, data, None, 8)
}

fn ensure_builtin_commands() {
    BUILTINS_INIT.get_or_init(|| {
        register_builtin(
            "help",
            Arc::new(|_args, registry| registry.help_text()),
            "h",
            "This help screen",
            INTERNAL_CMD_CLASS,
        );
        register_builtin(
            "hd",
            Arc::new(hex_dump_command),
            "",
            "Hex dump. Usage: hd <addr> [len=16]",
            INTERNAL_CMD_CLASS,
        );
    });
}

pub fn cmd_reg(
    id: impl Into<String>,
    handler: CommandHandler,
    shortcut: impl Into<String>,
    desc: impl Into<String>,
    group: impl Into<String>,
) -> bool {
    ensure_builtin_commands();
    global_commands()
        .lock()
        .unwrap()
        .register(id, handler, shortcut, desc, group)
}

pub fn command_registry_snapshot() -> CommandRegistry {
    ensure_builtin_commands();
    global_commands().lock().unwrap().clone()
}

pub fn hex_dump_bytes(
    ptr: *const u8,
    data: &[u8],
    name: Option<&str>,
    column_width: usize,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{ptr:p}  {}", name.unwrap_or("Data"));

    let columns = column_width.max(1);
    let mut address_abs = 0usize;
    for chunk in data.chunks(columns) {
        let _ = write!(out, "{address_abs:08x}");

        for idx in 0..columns {
            if idx % 8 == 0 {
                out.push(' ');
            }

            if let Some(byte) = chunk.get(idx) {
                let _ = write!(out, " {byte:02x}");
            } else {
                out.push_str("   ");
            }
        }

        out.push_str("  |");
        for byte in chunk {
            let ch = *byte as char;
            if ch.is_ascii_graphic() || ch == ' ' {
                out.push(ch);
            } else {
                out.push('.');
            }
        }
        out.push_str("|\n");

        address_abs += chunk.len();
    }

    out.trim_end().to_owned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParsedKey {
    Char(u8),
    Enter,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    JumpLeft,
    JumpRight,
    CtrlC,
    CtrlD,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParserState {
    Main,
    Escape,
    EscapeBracket,
    Escape1,
    Escape3,
    EscapeSemi,
    EscapeSemi5,
    Iac,
    IacOption,
}

#[derive(Default)]
pub struct SessionOutput {
    pub chunks: Vec<Vec<u8>>,
    pub disconnect: bool,
}

#[derive(Default)]
pub struct SystemCommanding {
    line: Vec<u8>,
    cursor: usize,
    history: Vec<String>,
    history_view: Option<usize>,
    last_command: Option<String>,
    mode_auto: bool,
    tab_last: bool,
    parser_state: ParserState,
    started: bool,
    term_changed: bool,
}

impl Default for ParserState {
    fn default() -> Self {
        Self::Main
    }
}

impl SystemCommanding {
    pub fn new() -> Self {
        ensure_builtin_commands();
        Self::default()
    }

    pub fn mode_auto_set(&mut self) {
        self.mode_auto = true;
    }

    pub fn last_command(&self) -> Option<&str> {
        self.last_command.as_deref()
    }

    pub fn on_connect(&mut self) -> Vec<Vec<u8>> {
        if self.started {
            return Vec::new();
        }
        self.started = true;

        if self.mode_auto {
            return Vec::new();
        }

        self.term_changed = true;
        vec![
            b"\xFF\xFB\x01\xFF\xFB\x03\xFF\xFC\x22\x1b[?25l\x1b[?1049h\x1b]2;SystemCommanding()\x07\x1b[2J\x1b[H"
                .to_vec(),
            WELCOME_MSG.as_bytes().to_vec(),
            self.prompt_bytes(),
        ]
    }

    pub fn disconnect_bytes(&self) -> Vec<u8> {
        if self.mode_auto {
            return Vec::new();
        }

        let mut out = Vec::from("\r\n".as_bytes());
        if self.term_changed {
            out.extend_from_slice(b"\x1b[?25h\x1b[?1049l");
        }
        out
    }

    pub fn ingest(&mut self, data: &[u8], registry: &CommandRegistry) -> SessionOutput {
        let mut output = SessionOutput::default();

        if self.mode_auto {
            for byte in data {
                match *byte {
                    b'\r' | b'\n' | 0 => {
                        let line = String::from_utf8_lossy(&self.line).trim().to_owned();
                        if !line.is_empty() {
                            self.last_command = Some(line.clone());
                        }
                        output
                            .chunks
                            .push(registry.execute_line(&line).into_bytes());
                        output.disconnect = true;
                        self.line.clear();
                        break;
                    }
                    0x03 | 0x04 => {
                        output.disconnect = true;
                        self.line.clear();
                        break;
                    }
                    byte => self.line.push(byte),
                }
            }

            return output;
        }

        for byte in data {
            let Some(key) = self.feed_byte(*byte) else {
                continue;
            };

            match key {
                ParsedKey::CtrlC | ParsedKey::CtrlD => {
                    output.disconnect = true;
                    break;
                }
                ParsedKey::Enter => self.execute_line(registry, &mut output),
                ParsedKey::Tab => self.tab_process(registry, &mut output),
                ParsedKey::Backspace => {
                    if self.cursor > 0 {
                        self.cursor -= 1;
                        self.line.remove(self.cursor);
                        self.tab_last = false;
                        output.chunks.push(self.prompt_bytes());
                    }
                }
                ParsedKey::Delete => {
                    if self.cursor < self.line.len() {
                        self.line.remove(self.cursor);
                        self.tab_last = false;
                        output.chunks.push(self.prompt_bytes());
                    }
                }
                ParsedKey::Left => {
                    if self.cursor > 0 {
                        self.cursor -= 1;
                        self.tab_last = false;
                        output.chunks.push(self.prompt_bytes());
                    }
                }
                ParsedKey::Right => {
                    if self.cursor < self.line.len() {
                        self.cursor += 1;
                        self.tab_last = false;
                        output.chunks.push(self.prompt_bytes());
                    }
                }
                ParsedKey::Home => {
                    if self.cursor != 0 {
                        self.cursor = 0;
                        self.tab_last = false;
                        output.chunks.push(self.prompt_bytes());
                    }
                }
                ParsedKey::End => {
                    if self.cursor != self.line.len() {
                        self.cursor = self.line.len();
                        self.tab_last = false;
                        output.chunks.push(self.prompt_bytes());
                    }
                }
                ParsedKey::Up => {
                    if self.history_up() {
                        output.chunks.push(self.prompt_bytes());
                    }
                }
                ParsedKey::Down => {
                    if self.history_down() {
                        output.chunks.push(self.prompt_bytes());
                    }
                }
                ParsedKey::JumpLeft => {
                    if self.jump_left() {
                        self.tab_last = false;
                        output.chunks.push(self.prompt_bytes());
                    }
                }
                ParsedKey::JumpRight => {
                    if self.jump_right() {
                        self.tab_last = false;
                        output.chunks.push(self.prompt_bytes());
                    }
                }
                ParsedKey::Char(ch) => {
                    self.line.insert(self.cursor, ch);
                    self.cursor += 1;
                    self.tab_last = false;
                    output.chunks.push(self.prompt_bytes());
                }
            }
        }

        output
    }

    fn execute_line(&mut self, registry: &CommandRegistry, output: &mut SessionOutput) {
        output.chunks.push(b"\r\n".to_vec());

        let line = String::from_utf8_lossy(&self.line).trim().to_owned();
        if !line.is_empty() {
            self.last_command = Some(line.clone());
            self.history_insert(&line);
            let response = registry.execute_line(&line);
            if !response.is_empty() {
                let mut normalized = normalize_lf_to_crlf(&response);
                normalized.push_str("\r\n");
                output.chunks.push(normalized.into_bytes());
            }
        }

        self.line.clear();
        self.cursor = 0;
        self.history_view = None;
        self.tab_last = false;
        output.chunks.push(self.prompt_bytes());
    }

    fn tab_process(&mut self, registry: &CommandRegistry, output: &mut SessionOutput) {
        if self.cursor == 0 || self.line[..self.cursor].contains(&b' ') {
            return;
        }

        let prefix = String::from_utf8_lossy(&self.line[..self.cursor]).to_string();
        let candidates = registry.candidates(&prefix);
        if candidates.is_empty() {
            self.tab_last = false;
            return;
        }

        if self.tab_last {
            let mut msg = String::from("\r\n");
            let mut col = 0usize;
            for candidate in &candidates {
                let truncated = &candidate[..candidate.len().min(20)];
                let _ = write!(msg, "{truncated:<22}");
                col += 1;
                if col >= 2 {
                    msg.push_str("\r\n");
                    col = 0;
                }
            }
            if col != 0 {
                msg.push_str("\r\n");
            }
            output.chunks.push(msg.into_bytes());
            output.chunks.push(self.prompt_bytes());
            return;
        }

        let mut common = candidates[0].to_owned();
        for candidate in candidates.iter().skip(1) {
            let common_len = common
                .bytes()
                .zip(candidate.bytes())
                .take_while(|(left, right)| left == right)
                .count();
            common.truncate(common_len);
        }

        if common.len() > prefix.len() {
            self.line = common.into_bytes();
            self.cursor = self.line.len();
            output.chunks.push(self.prompt_bytes());
        } else if candidates.len() == 1 && prefix == candidates[0] {
            self.line.push(b' ');
            self.cursor = self.line.len();
            output.chunks.push(self.prompt_bytes());
        }

        self.tab_last = true;
    }

    fn history_insert(&mut self, line: &str) {
        if self.history.last().is_some_and(|last| last == line) {
            return;
        }
        self.history.push(line.to_owned());
        if self.history.len() > HISTORY_MAX {
            let _ = self.history.remove(0);
        }
    }

    fn history_up(&mut self) -> bool {
        if self.history.is_empty() {
            return false;
        }

        self.history_view = Some(match self.history_view {
            None => self.history.len() - 1,
            Some(index) => index.saturating_sub(1),
        });

        self.load_history_view();
        true
    }

    fn history_down(&mut self) -> bool {
        let Some(index) = self.history_view else {
            return false;
        };

        if index + 1 >= self.history.len() {
            self.history_view = None;
            self.line.clear();
            self.cursor = 0;
            return true;
        }

        self.history_view = Some(index + 1);
        self.load_history_view();
        true
    }

    fn load_history_view(&mut self) {
        if let Some(index) = self.history_view {
            self.line = self.history[index].as_bytes().to_vec();
            self.cursor = self.line.len();
        }
        self.tab_last = false;
    }

    fn jump_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }

        let mut cursor = self.cursor;
        while cursor > 0 && self.line[cursor - 1] == b' ' {
            cursor -= 1;
        }
        while cursor > 0 && self.line[cursor - 1].is_ascii_alphanumeric() {
            cursor -= 1;
        }

        let changed = cursor != self.cursor;
        self.cursor = cursor;
        changed
    }

    fn jump_right(&mut self) -> bool {
        if self.cursor >= self.line.len() {
            return false;
        }

        let mut cursor = self.cursor;
        while cursor < self.line.len() && self.line[cursor].is_ascii_alphanumeric() {
            cursor += 1;
        }
        while cursor < self.line.len() && self.line[cursor] == b' ' {
            cursor += 1;
        }

        let changed = cursor != self.cursor;
        self.cursor = cursor;
        changed
    }

    fn prompt_bytes(&self) -> Vec<u8> {
        let mut msg = format!(
            "\r{PROMPT_PREFIX}{}\x1b[K",
            String::from_utf8_lossy(&self.line)
        );
        let tail = self.line.len().saturating_sub(self.cursor);
        if tail > 0 {
            let _ = write!(msg, "\x1b[{tail}D");
        }
        msg.into_bytes()
    }

    fn feed_byte(&mut self, byte: u8) -> Option<ParsedKey> {
        match self.parser_state {
            ParserState::Main => match byte {
                0xff => {
                    self.parser_state = ParserState::Iac;
                    None
                }
                0x1b => {
                    self.parser_state = ParserState::Escape;
                    None
                }
                0x03 => Some(ParsedKey::CtrlC),
                0x04 => Some(ParsedKey::CtrlD),
                b'\r' | b'\n' => Some(ParsedKey::Enter),
                b'\t' => Some(ParsedKey::Tab),
                0x7f | 0x08 => Some(ParsedKey::Backspace),
                0x20..=0x7e => Some(ParsedKey::Char(byte)),
                _ => None,
            },
            ParserState::Escape => {
                self.parser_state = ParserState::Main;
                if byte == b'[' {
                    self.parser_state = ParserState::EscapeBracket;
                }
                None
            }
            ParserState::EscapeBracket => match byte {
                b'A' => {
                    self.parser_state = ParserState::Main;
                    Some(ParsedKey::Up)
                }
                b'B' => {
                    self.parser_state = ParserState::Main;
                    Some(ParsedKey::Down)
                }
                b'C' => {
                    self.parser_state = ParserState::Main;
                    Some(ParsedKey::Right)
                }
                b'D' => {
                    self.parser_state = ParserState::Main;
                    Some(ParsedKey::Left)
                }
                b'F' | b'8' | b'4' => {
                    self.parser_state = ParserState::Main;
                    Some(ParsedKey::End)
                }
                b'H' | b'7' => {
                    self.parser_state = ParserState::Main;
                    Some(ParsedKey::Home)
                }
                b'1' => {
                    self.parser_state = ParserState::Escape1;
                    None
                }
                b'3' => {
                    self.parser_state = ParserState::Escape3;
                    None
                }
                _ => {
                    self.parser_state = ParserState::Main;
                    None
                }
            },
            ParserState::Escape1 => match byte {
                b'~' => {
                    self.parser_state = ParserState::Main;
                    Some(ParsedKey::Home)
                }
                b';' => {
                    self.parser_state = ParserState::EscapeSemi;
                    None
                }
                _ => {
                    self.parser_state = ParserState::Main;
                    None
                }
            },
            ParserState::Escape3 => {
                self.parser_state = ParserState::Main;
                (byte == b'~').then_some(ParsedKey::Delete)
            }
            ParserState::EscapeSemi => {
                self.parser_state = if byte == b'5' {
                    ParserState::EscapeSemi5
                } else {
                    ParserState::Main
                };
                None
            }
            ParserState::EscapeSemi5 => {
                self.parser_state = ParserState::Main;
                match byte {
                    b'C' => Some(ParsedKey::JumpRight),
                    b'D' => Some(ParsedKey::JumpLeft),
                    _ => None,
                }
            }
            ParserState::Iac => {
                self.parser_state = ParserState::IacOption;
                None
            }
            ParserState::IacOption => {
                self.parser_state = ParserState::Main;
                None
            }
        }
    }
}

fn normalize_lf_to_crlf(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            out.push('\r');
            if chars.peek() == Some(&'\n') {
                out.push('\n');
                chars.next();
            }
            continue;
        }

        if ch == '\n' {
            out.push_str("\r\n");
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_help_matches_cpp_style_sorting() {
        let mut registry = CommandRegistry::new();
        registry.register(
            "help",
            Arc::new(|_args, registry| registry.help_text()),
            "h",
            "This help screen",
            INTERNAL_CMD_CLASS,
        );
        registry.register(
            "zeta",
            Arc::new(|_args, _registry| String::new()),
            "",
            "last",
            "app",
        );
        registry.register(
            "alpha",
            Arc::new(|_args, _registry| String::new()),
            "a",
            "first",
            "app",
        );

        let help = registry.help_text();
        let help_pos = help.find("help").unwrap();
        let alpha_pos = help.find("alpha").unwrap();
        let zeta_pos = help.find("zeta").unwrap();

        assert!(help.contains("Available commands"));
        assert!(help_pos < alpha_pos);
        assert!(alpha_pos < zeta_pos);
    }

    #[test]
    fn interactive_session_supports_history_and_autocomplete() {
        let mut registry = CommandRegistry::new();
        registry.register(
            "help",
            Arc::new(|_args, registry| registry.help_text()),
            "h",
            "This help screen",
            INTERNAL_CMD_CLASS,
        );
        registry.register(
            "wave",
            Arc::new(|_args, _registry| String::from("world")),
            "",
            "test",
            "app",
        );

        let mut session = SystemCommanding::new();
        let connect = session.on_connect();
        assert!(!connect.is_empty());

        let out = session.ingest(b"wa\t\r", &registry);
        let output = String::from_utf8(out.chunks.concat()).unwrap();
        assert!(output.contains("world"));

        let history = session.ingest(b"\x1b[A\r", &registry);
        let history_text = String::from_utf8(history.chunks.concat()).unwrap();
        assert!(history_text.contains("world"));
    }

    #[test]
    fn auto_mode_executes_single_command() {
        let mut registry = CommandRegistry::new();
        registry.register(
            "echo",
            Arc::new(|args, _registry| args.to_owned()),
            "",
            "echoes",
            "app",
        );

        let mut session = SystemCommanding::new();
        session.mode_auto_set();
        let out = session.ingest(b"echo test\n", &registry);
        assert!(out.disconnect);
        assert_eq!(String::from_utf8(out.chunks.concat()).unwrap(), "test");
    }
}
