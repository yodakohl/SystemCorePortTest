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

mod logging;
mod net;
mod pipe;
mod processing;
mod single_wire;
mod system_commanding;
mod system_debugging;

pub use logging::{
    LogEntry, LogSeverity, level_log, level_log_set, subscribe_logs, user_debug, user_error,
    user_info, user_warn,
};
pub use net::{ReadStatus, TcpListening, TcpTransfering};
pub use pipe::{Pipe, PipeCommitStatus, PipeEntry, PipeRead, now_ms};
pub use processing::{
    DriverMode, ProcessBehavior, ProcessContext, ProcessHandle, ProcessRenderOptions, Success,
    application_close, drive_until_finished, num_burst_internal_drive_set,
    register_global_destructor, sleep_internal_drive_set,
};
pub use single_wire::{
    SingleWireFlowDirection, SingleWireFrameContent, SingleWireSchedulerContent,
    SingleWireTargetContent,
};
pub use system_commanding::{
    CommandRegistry, INTERNAL_CMD_CLASS, SystemCommand, SystemCommanding, cmd_reg,
    command_registry_snapshot, hex_dump_bytes,
};
pub use system_debugging::SystemDebugging;
