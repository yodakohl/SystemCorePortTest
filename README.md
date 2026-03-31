# SystemCore Rust Port

Rust port of `fractal-programming/SystemCore`.

This repository keeps the original project’s core model:

- recursive process trees driven by repeated ticks
- optional internal-driver threads for child processes
- success-based lifecycle (`Pending`, `Positive`, negative error codes)
- tree rendering for live inspection
- broadcast pipes
- TCP-based debugging endpoints

The port is intentionally Rust-native rather than a line-for-line transliteration of the C++ sources. The public surface is smaller, the memory model is owned/`Arc`-based, and host-side networking/debugging is implemented directly with `std`.

## Status

Implemented:

- `Processing` equivalent as `ProcessHandle` + `ProcessBehavior`
- internal-driver threads (`DriverMode::NewInternalDriver`)
- lifecycle flags and tree rendering
- global log fan-out
- generic `Pipe<T>`
- host-side `TcpListening` and `TcpTransfering`
- `SystemDebugging` with process-tree, log, and command sockets
- `tools/hello-world` equivalent as `examples/hello_world.rs`
- single-wire protocol enums from `SingleWire.h`

Not yet ported:

- ESP-IDF-specific `EspWifiConnecting`
- STM32 single-wire transport implementation
- the original telnet-style line editor from `SystemCommanding.cpp`

Those missing pieces are platform SDK integrations rather than core scheduler/runtime functionality, so the first Rust publish focuses on the portable host-side framework.

## Run

```bash
cargo test
cargo run --example hello_world
```

The hello-world example starts the debugger on:

- process tree: `localhost:3000`
- log stream: `localhost:3002`
- command stream: `localhost:3004`
- auto-command stream: `localhost:3006`

## Source Mapping

- `Processing.*` -> [`src/processing.rs`](./src/processing.rs)
- `Pipe.h` -> [`src/pipe.rs`](./src/pipe.rs)
- `TcpListening.*` / `TcpTransfering.*` -> [`src/net.rs`](./src/net.rs)
- `SystemDebugging.*` -> [`src/system_debugging.rs`](./src/system_debugging.rs)
- `SystemCommanding.*` -> [`src/system_commanding.rs`](./src/system_commanding.rs)
- `SingleWire.h` -> [`src/single_wire.rs`](./src/single_wire.rs)

## License

The upstream repository is MIT-licensed. This port keeps the same license and documents the original source repository in this README.
