# SystemCore Rust Port

Rust port of `fractal-programming/SystemCore`.

This repository keeps the original project’s core model:

- recursive process trees driven by repeated ticks
- optional internal-driver threads for child processes
- success-based lifecycle (`Pending`, `Positive`, negative error codes)
- tree rendering for live inspection
- broadcast pipes
- TCP-based debugging endpoints

The port is intentionally Rust-native rather than a line-for-line transliteration of the C++ sources. Ownership is `Arc`/`Mutex`-based instead of raw-pointer-based, but the host-side behavior is aligned against the original C++ implementation with direct conformance tests.

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
