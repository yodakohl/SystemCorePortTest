<!--
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
-->

# SystemCore Rust Port

Rust port of `fractal-programming/SystemCore`.

This repository keeps the original project’s core model:

- recursive process trees driven by repeated ticks
- optional internal-driver threads for child processes
- success-based lifecycle (`Pending`, `Positive`, negative error codes)
- tree rendering for live inspection
- broadcast pipes
- TCP-based debugging endpoints

The port is intentionally Rust-native rather than a line-for-line transliteration of the C++ sources. Ownership is `Arc`/`Mutex`-based instead of raw-pointer-based, but the host-side behavior is aligned against the original C++ implementation with direct conformance tests and side-by-side execution of the upstream `tools/hello-world` example.

## Status

Implemented:

- `Processing` equivalent as `ProcessHandle` + `ProcessBehavior`
- internal-driver threads (`DriverMode::NewInternalDriver`)
- lifecycle flags and tree rendering
- global log fan-out
- generic `Pipe<T>`
- host-side `TcpListening` and `TcpTransfering`
- global command registration plus interactive and auto command sockets
- `SystemDebugging` with process-tree, log, and command sockets
- `tools/hello-world` equivalent as `examples/hello_world.rs`
- single-wire protocol enums from `SingleWire.h`
- conformance tests for scheduler lifecycle, tree rendering, command behavior, and debugger socket flows

Validated directly against the original C++ hello-world binary:

- command welcome/help behavior on port `3004`
- auto-command behavior on port `3006`
- process-tree structure and details on port `3000`
- debugger listener layout and active-session rendering

Still not byte-for-byte equivalent:

- console and socket log formatting
- upstream core-log verbosity and source-location-rich log output

Not yet ported:

- ESP-IDF-specific `EspWifiConnecting`
- STM32 single-wire transport implementation

Those remaining gaps are platform SDK integrations rather than host-side runtime behavior.

## Run

```bash
cargo test
cargo run --example hello_world
```

The hello-world example starts the debugger on:

- process tree: `0.0.0.0:3000` and `[::]:3000`
- log stream: `0.0.0.0:3002` and `[::]:3002`
- command stream: `0.0.0.0:3004` and `[::]:3004`
- auto-command stream: `0.0.0.0:3006` and `[::]:3006`

## Source Mapping

- `Processing.*` -> [`src/processing.rs`](./src/processing.rs)
- `Pipe.h` -> [`src/pipe.rs`](./src/pipe.rs)
- `TcpListening.*` / `TcpTransfering.*` -> [`src/net.rs`](./src/net.rs)
- `SystemDebugging.*` -> [`src/system_debugging.rs`](./src/system_debugging.rs)
- `SystemCommanding.*` -> [`src/system_commanding.rs`](./src/system_commanding.rs)
- `SingleWire.h` -> [`src/single_wire.rs`](./src/single_wire.rs)

## License

The upstream repository is MIT-licensed. This port keeps the same license and documents the original source repository in this README.
