/*
  Derived from fractal-programming/SystemCore

  Copyright (c) 2023 Fractal Programming
  Copyright (c) 2026 yodakohl

  Permission is hereby granted, free of charge, to any person obtaining a copy
  of this software and associated documentation files (the "Software"), to deal
  in the Software without restriction, including without limitation the rights
  to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
  copies of the Software, and to permit persons to whom the Software is
  furnished to do so, subject to the following conditions:

  The above copyright notice and this permission notice shall be included in all
  copies or substantial portions of the Software.

  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
  IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
  FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
  AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
  LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
  OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
  SOFTWARE.
*/

use crate::logging::{LogSeverity, level_log_set, subscribe_logs};
use crate::net::{TcpListening, TcpTransfering, format_socket_addr};
use crate::processing::{
    ProcessBehavior, ProcessContext, ProcessHandle, ProcessRenderOptions, Success,
};
use crate::system_commanding::{
    INTERNAL_CMD_CLASS, SystemCommanding, cmd_reg, command_registry_snapshot,
};
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

const CTRL_C_TELNET_SEQ: &[u8] = b"\xff\xf4\xff\xfd\x06";

static SOCKET_LOG_LEVEL: AtomicUsize = AtomicUsize::new(LogSeverity::Info as usize);
static TREE_DETAILED: AtomicBool = AtomicBool::new(true);
static TREE_COLORED: AtomicBool = AtomicBool::new(true);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PeerType {
    ProcessTree,
    Log,
}

struct Peer {
    kind: PeerType,
    connection: TcpTransfering,
}

struct CommandPeer {
    connection: TcpTransfering,
    session: SystemCommanding,
    closing: bool,
}

pub struct SystemDebugging {
    root: ProcessHandle,
    listen_local: bool,
    port_start: u16,
    proc_listener: Option<TcpListening>,
    log_listener: Option<TcpListening>,
    cmd_listener: Option<TcpListening>,
    cmd_auto_listener: Option<TcpListening>,
    peers: Vec<Peer>,
    command_peers: Vec<CommandPeer>,
    log_rx: Option<Receiver<crate::logging::LogEntry>>,
    tree_cache: String,
    tree_dirty: bool,
    peer_log_once_connected: bool,
    update_period: Duration,
    last_tree_send: Instant,
}

impl SystemDebugging {
    pub fn new(root: ProcessHandle) -> Self {
        let _ = cmd_reg(
            "levelLog",
            std::sync::Arc::new(|args: &str, _registry| {
                let level = args.parse::<usize>().unwrap_or(3);
                level_log_set(level);
                format!("Log level set to {level}")
            }),
            "",
            "Set the log level for stdout",
            INTERNAL_CMD_CLASS,
        );

        let _ = cmd_reg(
            "levelLogSys",
            std::sync::Arc::new(|args: &str, _registry| {
                let level = args.parse::<usize>().unwrap_or(3);
                SOCKET_LOG_LEVEL.store(level, Ordering::Relaxed);
                format!("System log level set to {level}")
            }),
            "",
            "Set the log level for socket",
            INTERNAL_CMD_CLASS,
        );

        Self {
            root,
            listen_local: false,
            port_start: 3000,
            proc_listener: None,
            log_listener: None,
            cmd_listener: None,
            cmd_auto_listener: None,
            peers: Vec::new(),
            command_peers: Vec::new(),
            log_rx: None,
            tree_cache: String::new(),
            tree_dirty: true,
            peer_log_once_connected: false,
            update_period: Duration::from_millis(500),
            last_tree_send: Instant::now(),
        }
    }

    pub fn listen_local_set(&mut self) {
        self.listen_local = true;
    }

    pub fn port_start_set(&mut self, port: u16) {
        self.port_start = port;
    }

    pub fn ready(&self) -> bool {
        self.peer_log_once_connected
    }

    fn ensure_started(&mut self, ctx: &ProcessContext) -> Success {
        if self.proc_listener.is_some() {
            return Success::Positive;
        }

        let mut proc_listener = match TcpListening::bind(self.port_start, self.listen_local) {
            Ok(listener) => listener,
            Err(err) => return ctx.error(format!("failed to bind process tree listener: {err}")),
        };
        let log_listener = match TcpListening::bind(self.port_start + 2, self.listen_local) {
            Ok(listener) => listener,
            Err(err) => return ctx.error(format!("failed to bind log listener: {err}")),
        };
        let mut cmd_listener = match TcpListening::bind(self.port_start + 4, self.listen_local) {
            Ok(listener) => listener,
            Err(err) => return ctx.error(format!("failed to bind command listener: {err}")),
        };
        let mut cmd_auto_listener = match TcpListening::bind(self.port_start + 6, self.listen_local)
        {
            Ok(listener) => listener,
            Err(err) => return ctx.error(format!("failed to bind auto-command listener: {err}")),
        };

        proc_listener.max_conn_queued_set(200);
        cmd_listener.max_conn_queued_set(4);
        cmd_auto_listener.max_conn_queued_set(4);

        self.proc_listener = Some(proc_listener);
        self.log_listener = Some(log_listener);
        self.cmd_listener = Some(cmd_listener);
        self.cmd_auto_listener = Some(cmd_auto_listener);
        self.log_rx = Some(subscribe_logs());
        ctx.info(format!(
            "debug listeners ready on ports {}, {}, {}, {}",
            self.port_start,
            self.port_start + 2,
            self.port_start + 4,
            self.port_start + 6
        ));
        Success::Positive
    }

    fn accept_peers(&mut self) {
        if let Some(listener) = &mut self.proc_listener {
            let _ = listener.accept_ready();
            while let Some(connection) = listener.next_peer() {
                self.peers.push(Peer {
                    kind: PeerType::ProcessTree,
                    connection,
                });
                self.tree_dirty = true;
            }
        }

        if let Some(listener) = &mut self.log_listener {
            let _ = listener.accept_ready();
            while let Some(connection) = listener.next_peer() {
                self.peer_log_once_connected = true;
                self.peers.push(Peer {
                    kind: PeerType::Log,
                    connection,
                });
            }
        }

        if let Some(listener) = &mut self.cmd_listener {
            let _ = listener.accept_ready();
            while let Some(mut connection) = listener.next_peer() {
                let mut session = SystemCommanding::new();
                for chunk in session.on_connect() {
                    let _ = connection.queue_send(&chunk);
                }
                self.command_peers.push(CommandPeer {
                    connection,
                    session,
                    closing: false,
                });
            }
        }

        if let Some(listener) = &mut self.cmd_auto_listener {
            let _ = listener.accept_ready();
            while let Some(mut connection) = listener.next_peer() {
                let mut session = SystemCommanding::new();
                session.mode_auto_set();
                for chunk in session.on_connect() {
                    let _ = connection.queue_send(&chunk);
                }
                self.command_peers.push(CommandPeer {
                    connection,
                    session,
                    closing: false,
                });
            }
        }
    }

    fn disconnect_requested(bytes: &[u8]) -> bool {
        bytes
            .first()
            .is_some_and(|byte| *byte == 0x03 || *byte == 0x04)
            || bytes.starts_with(CTRL_C_TELNET_SEQ)
    }

    fn peer_check(&mut self) {
        self.peers.retain_mut(|peer| {
            let disconnect_requested = match peer.connection.read_available() {
                Ok(bytes) => Self::disconnect_requested(&bytes),
                Err(_) => true,
            };

            if disconnect_requested {
                peer.connection.done_set();
            }

            peer.connection.is_open()
        });
    }

    fn command_peers_pump(&mut self) {
        let registry = command_registry_snapshot();

        self.command_peers.retain_mut(|peer| {
            if peer.closing {
                if peer.connection.flush_pending().is_err() {
                    peer.connection.done_set();
                }
                if !peer.connection.has_pending_write() {
                    peer.connection.done_set();
                    return false;
                }
                return peer.connection.is_open();
            }

            let data = match peer.connection.read_available() {
                Ok(data) => data,
                Err(_) => {
                    peer.connection.done_set();
                    return false;
                }
            };

            if !data.is_empty() {
                let session_output = peer.session.ingest(&data, &registry);
                for chunk in session_output.chunks {
                    if peer.connection.queue_send(&chunk).is_err() {
                        peer.connection.done_set();
                        return false;
                    }
                }

                if session_output.disconnect {
                    let disconnect_bytes = peer.session.disconnect_bytes();
                    if !disconnect_bytes.is_empty() {
                        let _ = peer.connection.queue_send(&disconnect_bytes);
                    }
                    peer.closing = true;
                }
            }

            if peer.connection.flush_pending().is_err() {
                peer.connection.done_set();
                return false;
            }

            if peer.closing && !peer.connection.has_pending_write() {
                peer.connection.done_set();
                return false;
            }

            peer.connection.is_open()
        });
    }

    fn send_process_tree(&mut self) {
        let now = Instant::now();
        if !self.tree_dirty && now.duration_since(self.last_tree_send) < self.update_period {
            return;
        }

        let tree = self.root.process_tree_string(ProcessRenderOptions {
            detailed: TREE_DETAILED.load(Ordering::Relaxed),
            colored: TREE_COLORED.load(Ordering::Relaxed),
        });
        if tree == self.tree_cache && !self.tree_dirty {
            return;
        }

        let payload = format!("\x1b[2J\x1b[H{tree}");
        self.tree_cache = tree;
        self.tree_dirty = false;
        self.last_tree_send = now;

        self.peers.retain_mut(|peer| {
            if peer.kind != PeerType::ProcessTree {
                return peer.connection.is_open();
            }
            if peer.connection.queue_send(payload.as_bytes()).is_err() {
                peer.connection.done_set();
                return false;
            }
            if peer.connection.flush_pending().is_err() {
                peer.connection.done_set();
            }
            peer.connection.is_open()
        });
    }

    fn send_log_entries(&mut self) {
        let level = SOCKET_LOG_LEVEL.load(Ordering::Relaxed);
        let Some(rx) = &self.log_rx else {
            return;
        };

        while let Ok(entry) = rx.try_recv() {
            if entry.severity as usize > level {
                continue;
            }

            let process = entry.process.as_deref().unwrap_or("-");
            let message = format!(
                "{:>8}  {:<24} {:<3}  {}\r\n",
                entry.elapsed.as_millis(),
                process,
                entry.severity,
                entry.message
            );

            self.peers.retain_mut(|peer| {
                if peer.kind != PeerType::Log {
                    return peer.connection.is_open();
                }
                if peer.connection.queue_send(message.as_bytes()).is_err() {
                    peer.connection.done_set();
                    return false;
                }
                if peer.connection.flush_pending().is_err() {
                    peer.connection.done_set();
                }
                peer.connection.is_open()
            });
        }
    }

    fn push_virtual_child(
        lines: &mut Vec<String>,
        name: &str,
        details: impl IntoIterator<Item = String>,
    ) {
        lines.push(format!("- {name}()"));
        for detail in details {
            lines.push(format!("  {detail}"));
        }
    }

    fn listener_details(listener: &TcpListening) -> Vec<String> {
        vec![
            listener.address_summary(),
            format!("Connections created\t{}", listener.connections_created()),
            format!("Queue\t\t\t{}", listener.queue_len()),
        ]
    }

    fn transfer_details(connection: &TcpTransfering) -> Vec<String> {
        let mut details = vec![format!("Bytes received\t\t{}", connection.bytes_received())];

        if let (Some(local), Some(remote)) = (connection.addr_local(), connection.addr_remote()) {
            let mut endpoints = String::new();
            let _ = write!(
                endpoints,
                "{} <--> {}",
                format_socket_addr(&local),
                format_socket_addr(&remote)
            );
            details.push(endpoints);
        }

        details
    }

    fn process_tree_extra_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();

        if let Some(listener) = &self.proc_listener {
            Self::push_virtual_child(&mut lines, "TcpListening", Self::listener_details(listener));
        }
        if let Some(listener) = &self.log_listener {
            Self::push_virtual_child(&mut lines, "TcpListening", Self::listener_details(listener));
        }
        if let Some(listener) = &self.cmd_listener {
            Self::push_virtual_child(&mut lines, "TcpListening", Self::listener_details(listener));
        }
        if let Some(listener) = &self.cmd_auto_listener {
            Self::push_virtual_child(&mut lines, "TcpListening", Self::listener_details(listener));
        }

        for peer in &self.command_peers {
            Self::push_virtual_child(
                &mut lines,
                "SystemCommanding",
                [format!(
                    "Last command\t\t{}",
                    peer.session.last_command().unwrap_or("<none>")
                )],
            );
        }

        for peer in &self.peers {
            Self::push_virtual_child(
                &mut lines,
                "TcpTransfering",
                Self::transfer_details(&peer.connection),
            );
        }

        lines
    }
}

impl ProcessBehavior for SystemDebugging {
    fn name(&self) -> &str {
        "SystemDebugging"
    }

    fn process(&mut self, ctx: &mut ProcessContext) -> Success {
        let started = self.ensure_started(ctx);
        if started != Success::Positive {
            return started;
        }

        self.accept_peers();
        self.peer_check();
        self.command_peers_pump();
        ctx.current().refresh_render_cache_with_options(
            self,
            ProcessRenderOptions {
                detailed: TREE_DETAILED.load(Ordering::Relaxed),
                colored: TREE_COLORED.load(Ordering::Relaxed),
            },
        );
        self.send_process_tree();
        self.send_log_entries();

        Success::Pending
    }

    fn process_info(&self) -> Vec<String> {
        vec![format!(
            "Update period [ms]\t\t{}",
            self.update_period.as_millis()
        )]
    }

    fn process_tree_extra_lines(&self, _options: ProcessRenderOptions) -> Vec<String> {
        self.process_tree_extra_lines()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::user_info;
    use crate::processing::{DriverMode, ProcessContext, drive_until_finished};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::thread;

    fn read_until_contains(stream: &mut TcpStream, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut out = Vec::new();
        let mut buf = [0u8; 1024];

        while Instant::now() < deadline {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(len) => {
                    out.extend_from_slice(&buf[..len]);
                    if String::from_utf8_lossy(&out).contains(needle) {
                        break;
                    }
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::WouldBlock
                        || err.kind() == std::io::ErrorKind::TimedOut =>
                {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(err) => panic!("read failed: {err}"),
            }
        }

        String::from_utf8_lossy(&out).into_owned()
    }

    fn strip_ansi(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                    continue;
                }
                continue;
            }

            out.push(ch);
        }

        out
    }

    fn find_free_port_base() -> u16 {
        for base in (34000..38000).step_by(10) {
            let listeners: Option<Vec<_>> = [base, base + 2, base + 4, base + 6]
                .into_iter()
                .map(|port| TcpListener::bind(("127.0.0.1", port)).ok())
                .collect();
            if let Some(listeners) = listeners {
                drop(listeners);
                return base;
            }
        }
        panic!("no free port range found");
    }

    struct Root {
        started: bool,
        port_start: u16,
    }

    impl ProcessBehavior for Root {
        fn name(&self) -> &str {
            "Root"
        }

        fn process(&mut self, ctx: &mut ProcessContext) -> Success {
            if !self.started {
                self.started = true;
                let mut debugger = SystemDebugging::new(ctx.current());
                debugger.listen_local_set();
                debugger.port_start_set(self.port_start);
                ctx.start(ProcessHandle::new(debugger), DriverMode::Parent);
            }
            Success::Pending
        }
    }

    #[test]
    fn debugger_ports_expose_tree_logs_and_commands() {
        let base = find_free_port_base();
        let root = ProcessHandle::new(Root {
            started: false,
            port_start: base,
        });
        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = stop.clone();
        let root_bg = root.clone();

        let driver = thread::spawn(move || {
            while !stop_bg.load(Ordering::Relaxed) {
                root_bg.drive_for(4);
                thread::sleep(Duration::from_millis(5));
            }
            root_bg.unused_set();
            let _ = drive_until_finished(&root_bg, 4, Duration::from_millis(5));
        });

        thread::sleep(Duration::from_millis(150));

        let mut cmd = TcpStream::connect(("127.0.0.1", base + 4)).unwrap();
        cmd.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let welcome = read_until_contains(&mut cmd, "System Terminal");
        assert!(welcome.contains("System Terminal"));
        assert!(welcome.contains("core@app:~#"));

        cmd.write_all(b"help\r").unwrap();
        let help = read_until_contains(&mut cmd, "levelLogSys");
        assert!(help.contains("Available commands"));
        assert!(help.contains("levelLog"));
        assert!(help.contains("levelLogSys"));
        assert!(!help.contains("procTreeDetailedToggle"));
        assert!(!help.contains("procTreeColoredToggle"));

        let mut auto = TcpStream::connect(("127.0.0.1", base + 6)).unwrap();
        auto.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        auto.write_all(b"help\n").unwrap();
        let auto_help = read_until_contains(&mut auto, "Available commands");
        assert!(auto_help.contains("Available commands"));

        let mut tree = TcpStream::connect(("127.0.0.1", base)).unwrap();
        tree.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let tree_text = strip_ansi(&read_until_contains(&mut tree, "TcpListening()"));
        assert!(tree_text.contains("Root()"));
        assert!(tree_text.contains("SystemDebugging()"));
        assert!(tree_text.contains("Update period [ms]"));
        assert!(tree_text.contains("TcpListening()"));

        let mut log = TcpStream::connect(("127.0.0.1", base + 2)).unwrap();
        log.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        thread::sleep(Duration::from_millis(80));
        user_info("integration-log-line");
        let log_text = read_until_contains(&mut log, "integration-log-line");
        assert!(log_text.contains("integration-log-line"));

        stop.store(true, Ordering::Relaxed);
        driver.join().unwrap();
    }
}
