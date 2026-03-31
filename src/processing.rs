use crate::logging::{LogSeverity, emit};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverMode {
    Parent = 0,
    NewInternalDriver,
    ExternalDriver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Success {
    Pending,
    Positive,
    Negative(i16),
}

impl Success {
    pub fn is_pending(self) -> bool {
        matches!(self, Self::Pending)
    }

    pub fn is_positive(self) -> bool {
        matches!(self, Self::Positive)
    }

    pub fn code(self) -> i16 {
        match self {
            Self::Pending => 0,
            Self::Positive => 1,
            Self::Negative(code) => code,
        }
    }
}

pub trait ProcessBehavior: Send + 'static {
    fn name(&self) -> &str;

    fn initialize(&mut self, _ctx: &mut ProcessContext) -> Success {
        Success::Positive
    }

    fn process(&mut self, ctx: &mut ProcessContext) -> Success;

    fn shutdown(&mut self, _ctx: &mut ProcessContext) -> Success {
        Success::Positive
    }

    fn process_info(&self) -> Vec<String> {
        Vec::new()
    }

    fn process_tree_extra_lines(&self, _options: ProcessRenderOptions) -> Vec<String> {
        Vec::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProcessRenderOptions {
    pub detailed: bool,
    pub colored: bool,
}

impl Default for ProcessRenderOptions {
    fn default() -> Self {
        Self {
            detailed: true,
            colored: false,
        }
    }
}

#[derive(Clone)]
pub struct ProcessHandle {
    inner: Arc<ProcessNode>,
}

struct ProcessNode {
    name: String,
    runtime: Mutex<RuntimeState>,
    behavior: Mutex<Box<dyn ProcessBehavior>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AbstractState {
    Existent,
    Initializing,
    Processing,
    DownShutting,
    ChildrenUnusedSet,
    FinishedPrepare,
    Finished,
}

struct RuntimeState {
    children: Vec<ProcessHandle>,
    success: Success,
    state: AbstractState,
    level_tree: u8,
    level_driver: u8,
    driver_mode: DriverMode,
    started: bool,
    canceled: bool,
    unused: bool,
    when_finished_unused: bool,
    init_done: bool,
    process_done: bool,
    shutdown_done: bool,
    undriven: bool,
    tree_hidden: bool,
    info_cache: Vec<String>,
    extra_tree_cache: Vec<String>,
    worker: Option<JoinHandle<()>>,
}

impl RuntimeState {
    fn new() -> Self {
        Self {
            children: Vec::new(),
            success: Success::Pending,
            state: AbstractState::Existent,
            level_tree: 0,
            level_driver: 0,
            driver_mode: DriverMode::ExternalDriver,
            started: false,
            canceled: false,
            unused: false,
            when_finished_unused: false,
            init_done: false,
            process_done: false,
            shutdown_done: false,
            undriven: false,
            tree_hidden: false,
            info_cache: Vec::new(),
            extra_tree_cache: Vec::new(),
            worker: None,
        }
    }
}

#[derive(Clone, Copy)]
struct InternalDriveConfig {
    sleep: Duration,
    burst: usize,
}

impl Default for InternalDriveConfig {
    fn default() -> Self {
        Self {
            sleep: Duration::from_micros(2_000),
            burst: 13,
        }
    }
}

type Destructor = fn();

static INTERNAL_DRIVE: OnceLock<Mutex<InternalDriveConfig>> = OnceLock::new();
static GLOBAL_DESTRUCTORS: OnceLock<Mutex<Vec<Destructor>>> = OnceLock::new();

fn internal_drive_config() -> &'static Mutex<InternalDriveConfig> {
    INTERNAL_DRIVE.get_or_init(|| Mutex::new(InternalDriveConfig::default()))
}

fn global_destructors() -> &'static Mutex<Vec<Destructor>> {
    GLOBAL_DESTRUCTORS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn sleep_internal_drive_set(delay: Duration) {
    internal_drive_config().lock().unwrap().sleep = delay;
}

pub fn num_burst_internal_drive_set(burst: usize) {
    if burst > 0 {
        internal_drive_config().lock().unwrap().burst = burst;
    }
}

pub fn register_global_destructor(destructor: Destructor) {
    let mut destructors = global_destructors().lock().unwrap();
    if destructors
        .iter()
        .any(|registered| (*registered as usize) == destructor as usize)
    {
        return;
    }
    destructors.insert(0, destructor);
}

pub fn application_close() {
    let mut destructors = global_destructors().lock().unwrap();
    for destructor in destructors.drain(..) {
        destructor();
    }
}

pub fn drive_until_finished(root: &ProcessHandle, burst: usize, sleep: Duration) -> Success {
    while root.progress() {
        root.drive_for(burst.max(1));
        thread::sleep(sleep);
    }
    root.success()
}

pub struct ProcessContext {
    current: ProcessHandle,
}

impl ProcessContext {
    fn new(current: ProcessHandle) -> Self {
        Self { current }
    }

    pub fn current(&self) -> ProcessHandle {
        self.current.clone()
    }

    pub fn start(&mut self, child: ProcessHandle, driver: DriverMode) -> ProcessHandle {
        self.current.start_child(child, driver)
    }

    pub fn cancel(&mut self, child: &ProcessHandle) {
        child.mark_canceled();
    }

    pub fn repel(&mut self, child: &ProcessHandle) {
        child.mark_canceled();
        child.unused_set();
    }

    pub fn when_finished_repel(&mut self, child: &ProcessHandle) {
        child.mark_when_finished_unused();
    }

    pub fn unused_set(&mut self) {
        self.current.unused_set();
    }

    pub fn proc_tree_display_set(&mut self, display: bool) {
        self.current.proc_tree_display_set(display);
    }

    pub fn children_success(&self) -> Success {
        let children = self.current.children_snapshot();
        if children.is_empty() {
            return Success::Positive;
        }

        let mut one_pending = false;
        for child in children {
            if child.is_unused() {
                continue;
            }
            match child.success() {
                Success::Negative(code) => return Success::Negative(code),
                Success::Pending => one_pending = true,
                Success::Positive => {}
            }
        }

        if one_pending {
            Success::Pending
        } else {
            Success::Positive
        }
    }

    pub fn error(&self, message: impl Into<String>) -> Success {
        self.log(LogSeverity::Error, message);
        Success::Negative(-1)
    }

    pub fn warn(&self, message: impl Into<String>) {
        self.log(LogSeverity::Warn, message);
    }

    pub fn info(&self, message: impl Into<String>) {
        self.log(LogSeverity::Info, message);
    }

    pub fn debug(&self, message: impl Into<String>) {
        self.log(LogSeverity::Debug, message);
    }

    fn log(&self, severity: LogSeverity, message: impl Into<String>) {
        emit(severity, Some(self.current.name().as_str()), message);
    }
}

impl ProcessHandle {
    pub(crate) fn refresh_render_cache_with_options(
        &self,
        behavior: &dyn ProcessBehavior,
        options: ProcessRenderOptions,
    ) {
        let mut runtime = self.inner.runtime.lock().unwrap();
        runtime.info_cache = behavior.process_info();
        runtime.extra_tree_cache = behavior.process_tree_extra_lines(options);
    }

    pub fn new<B>(behavior: B) -> Self
    where
        B: ProcessBehavior,
    {
        let name = behavior.name().to_owned();
        Self {
            inner: Arc::new(ProcessNode {
                name,
                runtime: Mutex::new(RuntimeState::new()),
                behavior: Mutex::new(Box::new(behavior)),
            }),
        }
    }

    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    pub fn tree_tick(&self) {
        let children = self.children_snapshot();
        for child in children {
            self.parental_drive(&child);
            if child.can_be_removed() {
                self.remove_child(&child);
                child.join_worker();
            }
        }

        match self.abstract_state() {
            AbstractState::Existent => {
                if self.is_canceled() {
                    self.set_abstract_state(AbstractState::FinishedPrepare);
                } else {
                    self.set_abstract_state(AbstractState::Initializing);
                }
            }
            AbstractState::Initializing => {
                if self.is_canceled() {
                    self.set_abstract_state(AbstractState::DownShutting);
                    return;
                }

                let result = {
                    let mut behavior = self.inner.behavior.lock().unwrap();
                    let mut ctx = ProcessContext::new(self.clone());
                    let result = behavior.initialize(&mut ctx);
                    self.refresh_render_cache_with_options(
                        behavior.as_ref(),
                        ProcessRenderOptions::default(),
                    );
                    result
                };

                if result.is_pending() {
                    return;
                }

                match result {
                    Success::Positive => {
                        let mut runtime = self.inner.runtime.lock().unwrap();
                        runtime.init_done = true;
                        runtime.state = AbstractState::Processing;
                    }
                    Success::Negative(code) => {
                        let mut runtime = self.inner.runtime.lock().unwrap();
                        runtime.success = Success::Negative(code);
                        runtime.state = AbstractState::DownShutting;
                    }
                    Success::Pending => {}
                }
            }
            AbstractState::Processing => {
                if self.is_canceled() {
                    self.set_abstract_state(AbstractState::DownShutting);
                    return;
                }

                let result = {
                    let mut behavior = self.inner.behavior.lock().unwrap();
                    let mut ctx = ProcessContext::new(self.clone());
                    let result = behavior.process(&mut ctx);
                    self.refresh_render_cache_with_options(
                        behavior.as_ref(),
                        ProcessRenderOptions::default(),
                    );
                    result
                };

                if result.is_pending() {
                    return;
                }

                let mut runtime = self.inner.runtime.lock().unwrap();
                runtime.success = result;
                runtime.process_done = true;
                runtime.state = AbstractState::DownShutting;
            }
            AbstractState::DownShutting => {
                let result = {
                    let mut behavior = self.inner.behavior.lock().unwrap();
                    let mut ctx = ProcessContext::new(self.clone());
                    let result = behavior.shutdown(&mut ctx);
                    self.refresh_render_cache_with_options(
                        behavior.as_ref(),
                        ProcessRenderOptions::default(),
                    );
                    result
                };

                if result.is_pending() {
                    return;
                }

                let mut runtime = self.inner.runtime.lock().unwrap();
                runtime.shutdown_done = true;
                runtime.state = AbstractState::ChildrenUnusedSet;
            }
            AbstractState::ChildrenUnusedSet => {
                for child in self.children_snapshot() {
                    child.unused_set();
                }
                self.set_abstract_state(AbstractState::FinishedPrepare);
            }
            AbstractState::FinishedPrepare => {
                if self.is_when_finished_unused() {
                    self.unused_set();
                }
                self.set_abstract_state(AbstractState::Finished);
            }
            AbstractState::Finished => {}
        }
    }

    pub fn drive_for(&self, burst: usize) {
        for _ in 0..burst.max(1) {
            self.tree_tick();
        }
    }

    pub fn progress(&self) -> bool {
        let runtime = self.inner.runtime.lock().unwrap();
        runtime.state != AbstractState::Finished || !runtime.children.is_empty()
    }

    pub fn success(&self) -> Success {
        self.inner.runtime.lock().unwrap().success
    }

    pub fn unused_set(&self) {
        let mut runtime = self.inner.runtime.lock().unwrap();
        runtime.canceled = true;
        runtime.unused = true;
    }

    pub fn proc_tree_display_set(&self, display: bool) {
        self.inner.runtime.lock().unwrap().tree_hidden = !display;
    }

    pub fn init_done(&self) -> bool {
        self.inner.runtime.lock().unwrap().init_done
    }

    pub fn process_done(&self) -> bool {
        self.inner.runtime.lock().unwrap().process_done
    }

    pub fn shutdown_done(&self) -> bool {
        self.inner.runtime.lock().unwrap().shutdown_done
    }

    pub fn process_tree_string(&self, options: ProcessRenderOptions) -> String {
        let mut out = String::new();
        self.render_into(&mut out, options);
        out
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    fn render_into(&self, out: &mut String, options: ProcessRenderOptions) {
        let snapshot = {
            let runtime = self.inner.runtime.lock().unwrap();
            if runtime.tree_hidden {
                return;
            }
            (
                runtime.success,
                runtime.level_tree,
                runtime.level_driver,
                runtime.driver_mode,
                runtime.state,
                runtime.info_cache.clone(),
                runtime.extra_tree_cache.clone(),
                runtime.children.clone(),
            )
        };

        let (
            success,
            level_tree,
            level_driver,
            driver_mode,
            state,
            info_cache,
            extra_tree_cache,
            children,
        ) = snapshot;
        let marker = match success {
            Success::Pending => '-',
            Success::Positive => '+',
            Success::Negative(_) => 'x',
        };

        out.push_str(&" ".repeat(level_tree as usize * 2));
        out.push(marker);
        out.push(' ');

        let mut reset_color_after_name = false;
        match driver_mode {
            DriverMode::ExternalDriver => {
                if options.colored {
                    out.push_str("\x1b[38;5;135m");
                    reset_color_after_name = true;
                } else {
                    out.push_str("### ");
                }
            }
            DriverMode::NewInternalDriver => {
                if options.colored {
                    out.push_str("\x1b[38;5;81m");
                    reset_color_after_name = true;
                } else {
                    out.push_str("*** ");
                }
            }
            DriverMode::Parent => {}
        }

        if options.colored && level_driver == 0 {
            out.push_str("\x1b[38;5;40m");
            reset_color_after_name = true;
            out.push_str(&self.name());
            out.push_str("\x1b[0m");
        } else {
            out.push_str(&self.name());
        }
        out.push_str("()\r\n");
        if options.colored && reset_color_after_name {
            out.push_str("\x1b[37m");
        }

        if options.detailed && state != AbstractState::Finished {
            let (info_lines, extra_lines) = if let Ok(behavior) = self.inner.behavior.try_lock() {
                (
                    behavior.process_info(),
                    behavior.process_tree_extra_lines(options),
                )
            } else {
                (info_cache, extra_tree_cache)
            };

            for line in info_lines {
                out.push_str(&" ".repeat(level_tree as usize * 2 + 2));
                out.push_str(&line);
                out.push_str("\r\n");
            }

            for line in extra_lines {
                out.push_str(&" ".repeat(level_tree as usize * 2 + 2));
                out.push_str(&line);
                out.push_str("\r\n");
            }
        }

        let mut drawn = 0usize;
        for child in children {
            child.render_into(out, options);
            if !child.inner.runtime.lock().unwrap().tree_hidden {
                drawn += 1;
            }
            if drawn >= 11 {
                out.push_str(&" ".repeat(level_tree as usize * 2 + 2));
                out.push_str("..\r\n");
                break;
            }
        }
    }

    fn abstract_state(&self) -> AbstractState {
        self.inner.runtime.lock().unwrap().state
    }

    fn set_abstract_state(&self, state: AbstractState) {
        self.inner.runtime.lock().unwrap().state = state;
    }

    fn children_snapshot(&self) -> Vec<ProcessHandle> {
        self.inner.runtime.lock().unwrap().children.clone()
    }

    fn start_child(&self, child: ProcessHandle, driver: DriverMode) -> ProcessHandle {
        {
            let mut child_runtime = child.inner.runtime.lock().unwrap();
            if !child_runtime.started {
                let parent_runtime = self.inner.runtime.lock().unwrap();
                child_runtime.level_tree = parent_runtime.level_tree.saturating_add(1);
                child_runtime.driver_mode = driver;
                child_runtime.level_driver = match driver {
                    DriverMode::Parent => parent_runtime.level_driver,
                    DriverMode::NewInternalDriver | DriverMode::ExternalDriver => {
                        parent_runtime.level_driver.saturating_add(1)
                    }
                };
                child_runtime.started = true;
                child_runtime.undriven = false;
            }
        }

        {
            let mut runtime = self.inner.runtime.lock().unwrap();
            if !runtime
                .children
                .iter()
                .any(|existing| existing.ptr_eq(&child))
            {
                runtime.children.push(child.clone());
            }
        }

        if driver == DriverMode::NewInternalDriver {
            let should_spawn = child.inner.runtime.lock().unwrap().worker.is_none();
            if should_spawn {
                let thread_name = child.name();
                let worker_child = child.clone();
                let handle = thread::Builder::new()
                    .name(thread_name)
                    .spawn(move || internal_drive_loop(worker_child))
                    .ok();

                let mut runtime = child.inner.runtime.lock().unwrap();
                if let Some(handle) = handle {
                    runtime.worker = Some(handle);
                } else {
                    runtime.driver_mode = DriverMode::Parent;
                }
            }
        }

        child
    }

    fn remove_child(&self, child: &ProcessHandle) {
        let mut runtime = self.inner.runtime.lock().unwrap();
        runtime.children.retain(|existing| !existing.ptr_eq(child));
    }

    fn parental_drive(&self, child: &ProcessHandle) {
        let (driver_mode, undriven) = {
            let runtime = child.inner.runtime.lock().unwrap();
            (runtime.driver_mode, runtime.undriven)
        };

        if driver_mode != DriverMode::Parent || undriven {
            return;
        }

        child.tree_tick();
        if !child.progress() {
            child.mark_undriven();
        }
    }

    fn join_worker(&self) {
        let handle = self.inner.runtime.lock().unwrap().worker.take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    fn can_be_removed(&self) -> bool {
        let runtime = self.inner.runtime.lock().unwrap();
        runtime.undriven && runtime.unused
    }

    fn mark_canceled(&self) {
        self.inner.runtime.lock().unwrap().canceled = true;
    }

    fn mark_when_finished_unused(&self) {
        self.inner.runtime.lock().unwrap().when_finished_unused = true;
    }

    fn is_when_finished_unused(&self) -> bool {
        self.inner.runtime.lock().unwrap().when_finished_unused
    }

    fn is_canceled(&self) -> bool {
        self.inner.runtime.lock().unwrap().canceled
    }

    fn is_unused(&self) -> bool {
        self.inner.runtime.lock().unwrap().unused
    }

    fn mark_undriven(&self) {
        self.inner.runtime.lock().unwrap().undriven = true;
    }
}

fn internal_drive_loop(child: ProcessHandle) {
    loop {
        let cfg = *internal_drive_config().lock().unwrap();
        for _ in 0..cfg.burst.max(1) {
            child.tree_tick();
        }

        if !child.progress() {
            child.mark_undriven();
            break;
        }

        if cfg.sleep.is_zero() {
            continue;
        }
        thread::sleep(cfg.sleep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Instant;

    struct OneShot(bool);

    impl ProcessBehavior for OneShot {
        fn name(&self) -> &str {
            "OneShot"
        }

        fn process(&mut self, _ctx: &mut ProcessContext) -> Success {
            if self.0 {
                Success::Positive
            } else {
                self.0 = true;
                Success::Pending
            }
        }
    }

    struct Parent {
        child: Option<ProcessHandle>,
        started: bool,
    }

    impl ProcessBehavior for Parent {
        fn name(&self) -> &str {
            "Parent"
        }

        fn process(&mut self, ctx: &mut ProcessContext) -> Success {
            if !self.started {
                let child = ProcessHandle::new(OneShot(false));
                ctx.start(child.clone(), DriverMode::NewInternalDriver);
                self.child = Some(child);
                self.started = true;
                return Success::Pending;
            }

            match self.child.as_ref().unwrap().success() {
                Success::Positive => Success::Positive,
                _ => Success::Pending,
            }
        }
    }

    struct ServiceChild {
        stop_seen: bool,
    }

    impl ProcessBehavior for ServiceChild {
        fn name(&self) -> &str {
            "ServiceChild"
        }

        fn process(&mut self, _ctx: &mut ProcessContext) -> Success {
            Success::Pending
        }

        fn shutdown(&mut self, _ctx: &mut ProcessContext) -> Success {
            if !self.stop_seen {
                self.stop_seen = true;
                return Success::Pending;
            }
            Success::Positive
        }
    }

    struct Finisher {
        started: Instant,
    }

    impl ProcessBehavior for Finisher {
        fn name(&self) -> &str {
            "Finisher"
        }

        fn process(&mut self, _ctx: &mut ProcessContext) -> Success {
            if self.started.elapsed() >= Duration::from_millis(20) {
                Success::Positive
            } else {
                Success::Pending
            }
        }
    }

    struct CancelingParent {
        service: Option<ProcessHandle>,
        finisher: Option<ProcessHandle>,
        started: bool,
    }

    impl ProcessBehavior for CancelingParent {
        fn name(&self) -> &str {
            "CancelingParent"
        }

        fn process(&mut self, ctx: &mut ProcessContext) -> Success {
            if !self.started {
                self.started = true;
                self.service = Some(ctx.start(
                    ProcessHandle::new(ServiceChild { stop_seen: false }),
                    DriverMode::NewInternalDriver,
                ));
                self.finisher = Some(ctx.start(
                    ProcessHandle::new(Finisher {
                        started: Instant::now(),
                    }),
                    DriverMode::Parent,
                ));
                return Success::Pending;
            }

            match self.finisher.as_ref().unwrap().success() {
                Success::Positive => Success::Positive,
                Success::Negative(code) => Success::Negative(code),
                Success::Pending => Success::Pending,
            }
        }
    }

    #[test]
    fn lifecycle_reaches_finished_state() {
        let proc = ProcessHandle::new(OneShot(false));
        let success = drive_until_finished(&proc, 1, Duration::from_millis(1));
        assert_eq!(success, Success::Positive);
        assert!(proc.init_done());
        assert!(proc.process_done());
        assert!(proc.shutdown_done());
    }

    #[test]
    fn internal_driver_child_completes() {
        let proc = ProcessHandle::new(Parent {
            child: None,
            started: false,
        });
        let deadline = Instant::now() + Duration::from_secs(1);
        while proc.progress() && Instant::now() < deadline {
            proc.drive_for(2);
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(proc.success(), Success::Positive);
    }

    #[test]
    fn internal_driver_service_child_is_canceled_on_parent_shutdown() {
        let proc = ProcessHandle::new(CancelingParent {
            service: None,
            finisher: None,
            started: false,
        });
        let deadline = Instant::now() + Duration::from_secs(1);
        while proc.progress() && Instant::now() < deadline {
            proc.drive_for(4);
            thread::sleep(Duration::from_millis(2));
        }
        assert!(
            !proc.progress(),
            "process tree did not settle:\n{}",
            proc.process_tree_string(ProcessRenderOptions::default())
        );
        assert_eq!(proc.success(), Success::Positive);
    }

    struct HiddenChild;

    impl ProcessBehavior for HiddenChild {
        fn name(&self) -> &str {
            "HiddenChild"
        }

        fn process(&mut self, ctx: &mut ProcessContext) -> Success {
            ctx.proc_tree_display_set(false);
            Success::Pending
        }
    }

    struct ManyChildren {
        started: bool,
    }

    struct VisiblePendingChild;

    impl ProcessBehavior for VisiblePendingChild {
        fn name(&self) -> &str {
            "VisiblePendingChild"
        }

        fn process(&mut self, _ctx: &mut ProcessContext) -> Success {
            Success::Pending
        }
    }

    impl ProcessBehavior for ManyChildren {
        fn name(&self) -> &str {
            "ManyChildren"
        }

        fn process(&mut self, ctx: &mut ProcessContext) -> Success {
            if !self.started {
                self.started = true;
                for _ in 0..12 {
                    ctx.start(ProcessHandle::new(VisiblePendingChild), DriverMode::Parent);
                }
                ctx.start(ProcessHandle::new(HiddenChild), DriverMode::Parent);
            }
            Success::Pending
        }
    }

    #[test]
    fn process_tree_matches_cpp_child_cap_behavior() {
        let proc = ProcessHandle::new(ManyChildren { started: false });
        proc.drive_for(4);
        let tree = proc.process_tree_string(ProcessRenderOptions::default());
        assert_eq!(tree.matches("VisiblePendingChild()").count(), 11);
        assert!(tree.contains("..\r\n"));
        assert!(!tree.contains("HiddenChild()"));
    }
}
