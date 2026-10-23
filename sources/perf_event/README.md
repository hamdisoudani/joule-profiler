# perf_event metric source

Performance counter source for Joule Profiler, using the Linux perf_event subsystem.
This crate builds a `Source` for `joule-profiler-core` that collects hardware and software performance counters (CPU cycles, instructions, cache misses, branch mispredictions…) via the perf_event_open(2) syscall, per phase.

## Requirements

| Requirement | Details |
|---|---|
| OS | Linux kernel 2.6.31+ |
| CPU | Any architecture with PMU support (x86, ARM, RISC-V…) |
| Permissions | Root, or `kernel.perf_event_paranoid <= 1` |

### Adjusting perf_event_paranoid

```bash
# Check current value
cat /proc/sys/kernel/perf_event_paranoid
 
# Allow per-process counters for unprivileged users (temporary)
sudo sysctl kernel.perf_event_paranoid=1
```

## Scope

By default, Joule Profiler attaches `perf_event` counters to the **monitored process** only (per-process mode, via the pid), following it and any children it spawns across every CPU it runs on.

```toml
[sources.perf_event]
scope = "pid"   # the default
events = ["cpu_cycles", "instructions", "cache_misses", "branch_misses"] # the default
```

The available events are `cpu_cycles`, `instructions`, `cache_references`, `cache_misses`, `branch_instructions`, `branch_misses`, `bus_cycles` and `ref_cpu_cycles`.

Alternatively, counters can be scoped to a **cgroup v2** instead of the process's pid, with `scope = "cgroup"`:

```toml
[sources.perf_event]
scope = "cgroup"
name = "my-cgroup"        # relative to `root`, use "parent/child" for a nested cgroup
#root = "/sys/fs/cgroup"  # the default
#cpus = [0, 1]            # default: every online CPU
```

This tracks every process inside the cgroup rather than a single pid and its children, it can be useful for containers profiling, and lets counters be opened *before* the profiled process is spawned (the cgroup must already exist).