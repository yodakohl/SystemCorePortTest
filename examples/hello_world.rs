/*
  This file is part of the DSP-Crowd project
  https://www.dsp-crowd.com

  Author(s):
      - Johannes Natter, office@dsp-crowd.com

  File created on 14.09.2018

  Copyright (C) 2018, Johannes Natter

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

use std::thread;
use std::time::{Duration, Instant};

use systemcore::{
    DriverMode, ProcessBehavior, ProcessContext, ProcessHandle, Success, SystemDebugging,
    application_close, drive_until_finished, level_log_set,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChildState {
    Start,
    Main,
    Talk,
}

impl ChildState {
    fn label(self) -> &'static str {
        match self {
            Self::Start => "StStart",
            Self::Main => "StMain",
            Self::Talk => "StTalk",
        }
    }
}

struct ChildExecuting {
    state: ChildState,
    as_service: bool,
    delay_shutdown: bool,
    start: Option<Instant>,
    told_ya: bool,
}

impl ChildExecuting {
    fn new() -> Self {
        Self {
            state: ChildState::Start,
            as_service: false,
            delay_shutdown: false,
            start: None,
            told_ya: false,
        }
    }
}

impl ProcessBehavior for ChildExecuting {
    fn name(&self) -> &str {
        "ChildExecuting"
    }

    fn process(&mut self, ctx: &mut ProcessContext) -> Success {
        match self.state {
            ChildState::Start => {
                if !self.as_service {
                    ctx.info("I will wait some time.");
                    self.start = Some(Instant::now());
                    self.state = ChildState::Talk;
                } else {
                    self.state = ChildState::Main;
                }
                Success::Pending
            }
            ChildState::Main => Success::Pending,
            ChildState::Talk => {
                if self
                    .start
                    .is_some_and(|start| start.elapsed() < Duration::from_secs(5))
                {
                    return Success::Pending;
                }

                ctx.info("Waiting done.");
                Success::Positive
            }
        }
    }

    fn shutdown(&mut self, ctx: &mut ProcessContext) -> Success {
        if !self.delay_shutdown {
            ctx.warn("I am not used anymore!");
            return Success::Positive;
        }

        if !self.told_ya {
            self.told_ya = true;
            ctx.warn("I am not used anymore!");
            ctx.warn("I will delay my shutdown for one cycle.");
            return Success::Pending;
        }

        Success::Positive
    }

    fn process_info(&self) -> Vec<String> {
        vec![
            format!("State\t\t\t{}", self.state.label()),
            format!(
                "Service process\t\t{}",
                if self.as_service { "Yes" } else { "No" }
            ),
            format!(
                "Shutdown will be delayed\t{}",
                if self.delay_shutdown { "Yes" } else { "No" }
            ),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntroState {
    Start,
    Main,
}

impl IntroState {
    fn label(self) -> &'static str {
        match self {
            Self::Start => "StStart",
            Self::Main => "StMain",
        }
    }
}

struct Introducing {
    state: IntroState,
    threaded_child: Option<ProcessHandle>,
    debugger_started: bool,
}

impl Introducing {
    fn new() -> Self {
        Self {
            state: IntroState::Start,
            threaded_child: None,
            debugger_started: false,
        }
    }
}

impl ProcessBehavior for Introducing {
    fn name(&self) -> &str {
        "Introducing"
    }

    fn process(&mut self, ctx: &mut ProcessContext) -> Success {
        match self.state {
            IntroState::Start => {
                level_log_set(5);

                if !self.debugger_started {
                    let debugger = ProcessHandle::new(SystemDebugging::new(ctx.current()));
                    ctx.start(debugger, DriverMode::Parent);
                    self.debugger_started = true;
                }

                for idx in 0..3 {
                    let mut child = ChildExecuting::new();
                    child.as_service = idx != 1;
                    child.delay_shutdown = idx == 2;
                    let handle = ProcessHandle::new(child);

                    let driver = if idx == 0 {
                        DriverMode::NewInternalDriver
                    } else {
                        DriverMode::Parent
                    };
                    let handle = ctx.start(handle, driver);

                    if idx == 1 {
                        self.threaded_child = Some(handle);
                    }
                }

                ctx.info("Hello!");
                self.state = IntroState::Main;
                Success::Pending
            }
            IntroState::Main => match self.threaded_child.as_ref().map(ProcessHandle::success) {
                Some(Success::Positive) => Success::Positive,
                Some(Success::Negative(code)) => Success::Negative(code),
                _ => Success::Pending,
            },
        }
    }

    fn process_info(&self) -> Vec<String> {
        vec![format!("State\t\t\t{}", self.state.label())]
    }
}

fn main() {
    let app = ProcessHandle::new(Introducing::new());
    let success = drive_until_finished(&app, 12, Duration::from_millis(15));
    application_close();

    if !success.is_positive() {
        thread::sleep(Duration::from_millis(50));
        std::process::exit(1);
    }
}
