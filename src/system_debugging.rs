use crate::logging::{LogSeverity, level_log_set, subscribe_logs};
use crate::net::{TcpListening, TcpTransfering};
use crate::processing::{
    ProcessBehavior, ProcessContext, ProcessHandle, ProcessRenderOptions, Success,
};
use crate::system_commanding::{CommandHandler, CommandRegistry, SystemCommanding};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

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
    registry: CommandRegistry,
    log_rx: Option<Receiver<crate::logging::LogEntry>>,
    tree_cache: String,
    tree_dirty: bool,
    peer_log_once_connected: bool,
    update_period: Duration,
    last_tree_send: Instant,
    socket_log_level: Arc<AtomicUsize>,
    tree_detailed: Arc<AtomicBool>,
    tree_colored: Arc<AtomicBool>,
}

impl SystemDebugging {
    pub fn new(root: ProcessHandle) -> Self {
        let socket_log_level = Arc::new(AtomicUsize::new(LogSeverity::Info as usize));
        let tree_detailed = Arc::new(AtomicBool::new(true));
        let tree_colored = Arc::new(AtomicBool::new(true));
        let mut registry = CommandRegistry::new();

        registry.register(
            "levelLog",
            Arc::new(|args: &str| {
                let level = args.parse::<usize>().unwrap_or(3);
                level_log_set(level);
                format!("stdout log level set to {level}")
            }) as CommandHandler,
            "ll",
            "set the stdout log level",
            "dbg",
        );

        {
            let socket_log_level = socket_log_level.clone();
            registry.register(
                "levelLogSys",
                Arc::new(move |args| {
                    let level = args.parse::<usize>().unwrap_or(3);
                    socket_log_level.store(level, Ordering::Relaxed);
                    format!("socket log level set to {level}")
                }),
                "lls",
                "set the socket log level",
                "dbg",
            );
        }

        {
            let tree_detailed = tree_detailed.clone();
            registry.register(
                "treeDetailed",
                Arc::new(move |args| {
                    let enabled = match args.trim() {
                        "0" | "false" | "off" => false,
                        "1" | "true" | "on" => true,
                        _ => !tree_detailed.load(Ordering::Relaxed),
                    };
                    tree_detailed.store(enabled, Ordering::Relaxed);
                    format!(
                        "detailed tree output {}",
                        if enabled { "enabled" } else { "disabled" }
                    )
                }),
                "td",
                "toggle detailed tree output",
                "dbg",
            );
        }

        {
            let tree_colored = tree_colored.clone();
            registry.register(
                "treeColored",
                Arc::new(move |args| {
                    let enabled = match args.trim() {
                        "0" | "false" | "off" => false,
                        "1" | "true" | "on" => true,
                        _ => !tree_colored.load(Ordering::Relaxed),
                    };
                    tree_colored.store(enabled, Ordering::Relaxed);
                    format!(
                        "colored tree output {}",
                        if enabled { "enabled" } else { "disabled" }
                    )
                }),
                "tc",
                "toggle colored tree output",
                "dbg",
            );
        }

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
            registry,
            log_rx: None,
            tree_cache: String::new(),
            tree_dirty: true,
            peer_log_once_connected: false,
            update_period: Duration::from_millis(500),
            last_tree_send: Instant::now(),
            socket_log_level,
            tree_detailed,
            tree_colored,
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
            while let Some(connection) = listener.next_peer() {
                self.command_peers.push(CommandPeer {
                    connection,
                    session: SystemCommanding::new(),
                });
            }
        }

        if let Some(listener) = &mut self.cmd_auto_listener {
            let _ = listener.accept_ready();
            while let Some(connection) = listener.next_peer() {
                let mut session = SystemCommanding::new();
                session.mode_auto_set();
                self.command_peers.push(CommandPeer {
                    connection,
                    session,
                });
            }
        }
    }

    fn command_peers_pump(&mut self) {
        self.command_peers.retain_mut(|peer| {
            let data = match peer.connection.read_available() {
                Ok(data) => data,
                Err(_) => {
                    peer.connection.done_set();
                    return false;
                }
            };

            if !data.is_empty() {
                for response in peer.session.ingest(&data, &self.registry) {
                    let mut line = response;
                    line.push_str("\r\n");
                    if peer.connection.queue_send(line.as_bytes()).is_err() {
                        peer.connection.done_set();
                        return false;
                    }
                }
            }

            if peer.connection.flush_pending().is_err() {
                peer.connection.done_set();
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
            detailed: self.tree_detailed.load(Ordering::Relaxed),
            colored: self.tree_colored.load(Ordering::Relaxed),
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
        let level = self.socket_log_level.load(Ordering::Relaxed);
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
        self.command_peers_pump();
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
}
