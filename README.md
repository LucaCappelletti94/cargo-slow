# slow-rs

A comprehensive system slowness diagnostic tool for Linux workstations. Designed to help identify the root cause of mysterious system slowdowns by continuously monitoring and benchmarking various system metrics.

## Features

### Live Benchmarks

- **I/O Read Speed**: Reads a test file and measures throughput (MB/s, enabled with `--io-bench`)
- **I/O Write Speed**: Writes to disk and measures throughput (MB/s, enabled with `--io-bench`)
- **SHA256 Read+Hash**: Measures combined I/O + CPU performance when `--io-bench` is enabled
- **Memory Allocation**: Benchmarks memory allocation speed
- **Compute Benchmark**: Pure CPU SHA256 hashing performance

### System Metrics

- **CPU**: Usage %, per-state breakdown (user/system/iowait/irq/steal), load averages
- **Memory**: Total/used/free/available, buffers, cached, swap, dirty pages, writeback
- **Disk I/O**: Reads/writes completed, sectors read/written, I/O time, queue depth
- **Network**: RX/TX bytes, packets, errors
- **Processes**: Count, running, blocked
- **Temperatures**: CPU, RAM (DIMM), and disk temperatures (via hwmon)
- **PSI (Pressure Stall Information)**: CPU/memory/IO pressure metrics
- **VM Stats**: Page faults (minor & major), swap in/out, page in/out
- **File Descriptors**: Allocated vs max
- **SMART Health**: Disk health status, reallocated sectors, pending sectors (requires sudo)

### Dynamic Monitoring Features

- **Severity-Based Highlighting**: Chart borders change color based on metric severity:
  - **Normal** (white): Metric is within acceptable range
  - **Warning** (yellow): Metric approaching problematic levels
  - **Critical** (red): Immediate attention needed

- **Actionable Recommendations**: When issues are detected, the UI shows specific advice:
  - High I/O pressure: "Check: iotop, iostat -x 1, dmesg for disk errors"
  - Swap activity: "Check: ps aux --sort=-%mem"
  - High temperatures: "Check cooling, clean dust, verify thermal paste"
  - Low memory: "Kill processes or add RAM immediately"

- **Availability Warnings**: Shows which metrics are unavailable and why:
  - PSI unavailable (requires Linux 4.20+ with CONFIG_PSI)
  - RAM temp sensors not found (no jc42 hwmon devices)
  - smartctl not found (install smartmontools for disk health)

### Keyboard Controls

| Key | Action |
|-----|--------|
| `q` | Quit |
| `Esc` | Quit |
| `Ctrl+C` | Quit |

### TUI Dashboard (9 Charts)

The dashboard displays a 3x3 grid of charts:

| Row | Left Chart | Middle Chart | Right Chart |
|-----|------------|--------------|-------------|
| 1 | I/O Read Speed (MB/s) | I/O Write Speed (MB/s) | CPU Usage (%) |
| 2 | Memory Available (MB) | I/O Pressure (avg10) | CPU Temperature (C) |
| 3 | RAM Temperature (C) | Disk Temperature (C) | IPMI DIMM Temperature (C) |

### Output

- **TUI Mode**: Real-time terminal UI with charts, highlighting, and recommendations
- **Headless Mode**: Simple logging to stdout
- **CSV Logging**: All metrics logged to CSV for analysis

## Installation

```bash
cargo build --release
# Binary will be at target/release/slow-rs
```

## Usage

```bash
# Run with TUI (default)
./target/release/slow-rs

# Run with sudo for full metrics (SMART disk health, IPMI/BMC sensors)
sudo ./target/release/slow-rs

# Run in headless mode (good for remote sessions or logging)
./target/release/slow-rs --headless

# Custom interval (default 5 seconds)
./target/release/slow-rs -i 10

# Enable I/O benchmark for disk throughput testing
./target/release/slow-rs --io-bench

# Full options
./target/release/slow-rs --help
```

### Running with Elevated Privileges

Some metrics require root access:

| Feature | Without sudo | With sudo |
|---------|--------------|-----------|
| CPU/Memory/Disk stats | Yes | Yes |
| Temperature (hwmon) | Yes | Yes |
| PSI pressure metrics | Yes | Yes |
| SMART disk health | No | Yes |
| IPMI/BMC sensors | No | Yes |
The UI will show a warning bar when metrics are unavailable due to permissions.

### IPMI/BMC Support

If your system has a BMC (Baseboard Management Controller), slow-rs can read IPMI DIMM temperature sensors with status (ok, nc, cr, nr).

IPMI sensor status meanings:
- **ok**: Normal operation
- **nc**: Non-Critical (warning threshold exceeded)
- **cr**: Critical (critical threshold exceeded)
- **nr**: Non-Recoverable (system may shut down to prevent damage)

If any DIMM is in `nr` or `cr` state, slow-rs will show a critical recommendation.

### Command Line Options

| Option | Description | Default |
|--------|-------------|---------|
| `-i, --interval` | Seconds between measurements | 5 |
| `-c, --csv-file` | Path to CSV log file | metrics.csv |
| `-t, --test-file` | Path to I/O test file | /tmp/slowtest.bin |
| `-f, --file-size-mb` | Size of test file in MB | 256 |
| `--history-size` | Data points to keep for plotting | 120 |
| `--headless` | Run without TUI | false |
| `--io-bench` | Enable I/O benchmark | false |

## Interpreting Results

### Severity Thresholds

The following thresholds trigger warning/critical highlighting:

| Metric | Warning | Critical |
|--------|---------|----------|
| I/O Pressure (avg10) | >= 10% | >= 25% |
| CPU Usage | >= 80% | >= 95% |
| Memory Available | <= 1024 MB | <= 256 MB |
| CPU Temperature | >= 75C | >= 85C |
| RAM (DIMM) Temperature | >= 70C | >= 80C |
| Disk Temperature | >= 50C | >= 60C |
| I/O Wait | >= 20% | >= 40% |

### Signs of Different Problems

**Disk Failure / I/O Issues:**

- Low I/O read/write speeds
- High I/O pressure (io_pressure_some_avg10 > 10%) - triggers yellow/red highlighting
- High disk_weighted_io_time_ms
- High cpu_iowait (> 20% triggers recommendation)
- Many pgmajfault (major page faults = disk reads)
- SMART: reallocated sectors > 0, pending sectors > 0

**Memory Pressure:**

- High mem_pressure values
- Swap usage increasing
- High pswpin/pswpout (swap activity) - triggers recommendation
- Low mem_available_mb (< 1GB triggers warning, < 256MB critical)
- High dirty pages or writeback (> 1GB triggers recommendation)

**CPU Issues:**

- High cpu_steal (VM being throttled)
- High temperatures (CPU > 85C triggers critical)
- Low compute benchmark scores
- High load averages with low CPU usage (suggests I/O wait)

**Thermal Throttling:**

- CPU temperatures > 75-85C (highlighted in yellow/red)
- RAM temperatures > 70-80C (highlighted in yellow/red)
- Disk temperatures > 50-60C (highlighted in yellow/red)
- Decreasing benchmark scores over time

**General Software Issues:**

- High process count
- High context_switches
- FD count approaching fd_max

## Analyzing Logged Data

The CSV file can be analyzed with pandas, Excel, or other tools:

```python
import pandas as pd
import matplotlib.pyplot as plt

df = pd.read_csv('metrics.csv')
df['datetime'] = pd.to_datetime(df['datetime'])

# Plot I/O read speed over time
plt.figure(figsize=(12, 6))
plt.plot(df['datetime'], df['io_read_mb_per_sec'])
plt.title('I/O Read Speed Over Time')
plt.ylabel('MB/s')
plt.xlabel('Time')
plt.savefig('io_speed.png')
```

## Requirements

- Linux (uses `/proc` filesystem)
- Rust 1.70+ (for building)

### Optional Dependencies

| Tool | Purpose | Install |
|------|---------|---------|
| `smartmontools` | SMART disk health monitoring | `apt install smartmontools` |
| `ipmitool` | Server BMC/IPMI sensors | `apt install ipmitool` |

### Hardware Sensors

- **CPU Temperature**: Requires `coretemp`, `k10temp`, or `zenpower` kernel modules
- **RAM Temperature**: Requires `jc42` kernel module (SPD temperature sensors on DIMMs)
- **Disk Temperature**: NVMe drives expose via hwmon; SATA drives require SMART

## License

MIT
