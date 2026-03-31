use std::sync::Arc;

pub type CommandHandler = Arc<dyn Fn(&str) -> String + Send + Sync>;

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
    ) {
        let command = SystemCommand {
            id: id.into(),
            shortcut: shortcut.into(),
            desc: desc.into(),
            group: group.into(),
            handler,
        };
        self.commands.retain(|existing| existing.id != command.id);
        self.commands.push(command);
        self.commands.sort_by(|left, right| left.id.cmp(&right.id));
    }

    pub fn execute_line(&self, line: &str) -> String {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let id = parts.next().unwrap_or_default();
        let args = parts.next().unwrap_or_default().trim();

        if id == "help" || id == "h" {
            return self.help_text();
        }

        let Some(command) = self.commands.iter().find(|command| {
            command.id == id || (!command.shortcut.is_empty() && command.shortcut == id)
        }) else {
            return format!("unknown command: {id}");
        };

        (command.handler)(args)
    }

    pub fn help_text(&self) -> String {
        let mut out = String::from("available commands:\n");
        out.push_str("  help / h                show this help\n");
        for command in &self.commands {
            let shortcut = if command.shortcut.is_empty() {
                String::new()
            } else {
                format!(" / {}", command.shortcut)
            };
            let group = if command.group.is_empty() {
                String::new()
            } else {
                format!(" [{}]", command.group)
            };
            out.push_str(&format!(
                "  {}{shortcut:<6} {group} {}\n",
                command.id, command.desc
            ));
        }
        out.trim_end().to_owned()
    }
}

#[derive(Default)]
pub struct SystemCommanding {
    buffer: String,
    mode_auto: bool,
}

impl SystemCommanding {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mode_auto_set(&mut self) {
        self.mode_auto = true;
    }

    pub fn ingest(&mut self, data: &[u8], registry: &CommandRegistry) -> Vec<String> {
        let mut responses = Vec::new();
        self.buffer.push_str(&String::from_utf8_lossy(data));

        while let Some(line_end) = self
            .buffer
            .find(|ch| ch == '\n' || ch == '\r' || ch == '\0')
        {
            let line = self.buffer[..line_end].trim().to_owned();
            let remaining = self.buffer[line_end + 1..].to_owned();
            self.buffer = remaining;

            if line.is_empty() {
                continue;
            }

            let response = registry.execute_line(&line);
            if !response.is_empty() {
                responses.push(response);
            }

            if self.mode_auto {
                self.buffer.clear();
                break;
            }
        }

        responses
    }
}
