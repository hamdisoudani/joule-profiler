# Joule Profiler library usage example

This crate provides an example of usage of Joule Profiler library: it measures the RAPL energy of a
small shell command, splits it into phases on the tokens it prints, and prints each phase in the
terminal.

Run the example:
```bash
cargo run -p library_usage_example
```

Reading RAPL through perf needs `kernel.perf_event_paranoid` set to 0 or lower, or root.
