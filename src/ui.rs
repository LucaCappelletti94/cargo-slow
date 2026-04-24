//! Terminal User Interface for slow-rs.
//!
//! This module provides a real-time dashboard using `ratatui` that displays:
//!
//! - Status bar with current metrics summary
//! - Nine charts showing key metrics over time
//! - Detailed metrics panels at the bottom
//!
//! # Controls
//!
//! - `q` or `Esc`: Quit

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::Span,
    widgets::{
        Axis, Block, BorderType, Borders, Chart, Dataset, GraphType, LegendPosition, List,
        ListItem, Paragraph, Wrap,
    },
    Frame, Terminal,
};

use crate::app::App;
use crate::availability::MetricAvailability;
use crate::metrics::Metrics;
use crate::recommendations::{generate_recommendations, Recommendation};
use crate::thresholds::{Severity, Thresholds};

const MAX_WARNING_LINES: usize = 3;
const MAX_RECOMMENDATION_LINES: usize = 4;

/// Run the TUI event loop.
///
/// This takes ownership of the App and terminal, running until the user
/// presses `q` or `Esc`, or the `running` flag is set to false.
///
/// # Arguments
///
/// * `app` - Application instance
/// * `running` - Atomic flag to signal shutdown
/// * `interval` - Time between metric collections
pub fn run(mut app: App, running: Arc<AtomicBool>, interval: Duration) -> std::io::Result<()> {
    let history_size = app.config.history_size;

    enable_raw_mode()?;
    if let Err(e) = std::io::stdout().execute(EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(e);
    }

    let result = run_tui_loop(&mut app, &running, interval, history_size);

    // Always clean up terminal state
    let _ = disable_raw_mode();
    let _ = std::io::stdout().execute(LeaveAlternateScreen);

    result
}

/// Inner TUI loop - separated to ensure cleanup happens on any exit path.
fn run_tui_loop(
    app: &mut App,
    running: &Arc<AtomicBool>,
    interval: Duration,
    history_size: usize,
) -> std::io::Result<()> {
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut last_collection = Instant::now();

    // Draw loading screen immediately so user sees something
    terminal.draw(|f| {
        draw_loading_screen(f);
    })?;

    // Initial collection (this is the slow part)
    if let Ok(metrics) = app.collect_metrics() {
        add_metrics(&mut app.metrics_history, metrics, history_size);
    }

    while running.load(Ordering::Relaxed) {
        // Check for input
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            running.store(false, Ordering::Relaxed);
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            running.store(false, Ordering::Relaxed);
                        }
                        _ => {}
                    }
                }
            }
        }

        // Collect metrics at interval
        if last_collection.elapsed() >= interval {
            if let Ok(metrics) = app.collect_metrics() {
                add_metrics(&mut app.metrics_history, metrics, history_size);
            }
            last_collection = Instant::now();
        }

        // Draw UI
        terminal.draw(|f| draw_ui(f, &app.metrics_history, &app.availability, &app.thresholds))?;
    }

    Ok(())
}

/// Add metrics to history, maintaining max size.
fn add_metrics(history: &mut VecDeque<Metrics>, metrics: Metrics, max_size: usize) {
    if history.len() >= max_size {
        history.pop_front();
    }
    history.push_back(metrics);
}

/// Main UI drawing function.
fn draw_ui(
    f: &mut Frame,
    metrics_history: &VecDeque<Metrics>,
    availability: &MetricAvailability,
    thresholds: &Thresholds,
) {
    let size = f.area();

    // Check if we have warnings to show
    let warnings = availability.get_warnings();
    let has_warnings = !warnings.is_empty();

    // Generate recommendations from latest metrics
    let recommendations = metrics_history
        .back()
        .map(|m| generate_recommendations(m, thresholds))
        .unwrap_or_default();
    let has_recommendations = !recommendations.is_empty();

    // Main layout: status bar, optional alerts, charts, optional advice, details.
    let mut constraints = vec![Constraint::Length(3)];
    if has_warnings {
        constraints.push(Constraint::Length(notification_panel_height(
            warnings.len(),
            MAX_WARNING_LINES,
        )));
    }
    constraints.push(Constraint::Min(18));
    if has_recommendations {
        constraints.push(Constraint::Length(notification_panel_height(
            recommendations.len(),
            MAX_RECOMMENDATION_LINES,
        )));
    }
    constraints.push(Constraint::Length(10));

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(size);

    let mut chunk_idx = 0;

    // Status bar
    draw_status_bar(f, metrics_history, main_chunks[chunk_idx]);
    chunk_idx += 1;

    // Warnings bar (if present)
    if has_warnings {
        draw_warnings(f, &warnings, main_chunks[chunk_idx]);
        chunk_idx += 1;
    }

    // Charts
    draw_charts(f, metrics_history, thresholds, main_chunks[chunk_idx]);
    chunk_idx += 1;

    // Recommendations (if present)
    if has_recommendations {
        draw_recommendations(f, &recommendations, main_chunks[chunk_idx]);
        chunk_idx += 1;
    }

    // Details
    draw_details(f, metrics_history, main_chunks[chunk_idx]);
}

/// Draw a loading screen while initial metrics are being collected.
fn draw_loading_screen(f: &mut Frame) {
    let size = f.area();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title("slow-rs")
        .border_style(Style::default().fg(Color::Cyan));

    let text = [
        "",
        "  Collecting initial metrics...",
        "",
        "  This includes:",
        "    - I/O benchmarks (read/write speed)",
        "    - Memory allocation tests",
        "    - CPU compute benchmarks",
        "    - SMART disk health (if available)",
        "    - IPMI/BMC sensors (if available)",
        "",
        "  Please wait...",
    ]
    .join("\n");

    let paragraph = Paragraph::new(text)
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(block);

    f.render_widget(paragraph, size);
}

/// Draw the top status bar.
fn draw_status_bar(f: &mut Frame, metrics_history: &VecDeque<Metrics>, area: Rect) {
    let status_text = if let Some(m) = metrics_history.back() {
        format!(
            " 📊 slow-rs | {} | CPU: {:.1}% | Mem: {}/{} MB | Load: {:.2} {:.2} {:.2} | Samples: {} | [q]uit",
            m.datetime,
            m.cpu_usage_percent,
            m.mem_used_mb,
            m.mem_total_mb,
            m.load_avg_1,
            m.load_avg_5,
            m.load_avg_15,
            metrics_history.len()
        )
    } else {
        " 📊 slow-rs | Collecting initial metrics... | [q]uit".to_string()
    };

    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title("Status"),
        );

    f.render_widget(status, area);
}

/// Draw the warnings bar for unavailable metrics.
fn draw_warnings(f: &mut Frame, warnings: &[String], area: Rect) {
    let lines = warnings
        .iter()
        .map(|warning| format!("! {}", warning))
        .collect();
    let text = join_limited_lines(lines, MAX_WARNING_LINES);
    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(Color::Black).bg(Color::Yellow))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title("⚠ Limited Metrics")
                .border_style(Style::default().fg(Color::Yellow)),
        );
    f.render_widget(paragraph, area);
}

/// Draw the recommendations panel showing ALL critical issues.
fn draw_recommendations(f: &mut Frame, recommendations: &[Recommendation], area: Rect) {
    if recommendations.is_empty() {
        return;
    }

    // Collect all critical and warning recommendations
    let critical: Vec<_> = recommendations
        .iter()
        .filter(|r| r.severity == Severity::Critical)
        .collect();
    let warnings: Vec<_> = recommendations
        .iter()
        .filter(|r| r.severity == Severity::Warning)
        .collect();

    // Build text showing all critical issues first, then warnings
    let mut lines = Vec::new();
    for rec in &critical {
        lines.push(format!("🔴 {} - {}", rec.title, rec.advice));
    }
    for rec in &warnings {
        lines.push(format!("🟡 {} - {}", rec.title, rec.advice));
    }

    let text = join_limited_lines(lines, MAX_RECOMMENDATION_LINES);

    // Use most severe color for the panel
    let (fg, bg, border_color) = if !critical.is_empty() {
        (Color::White, Color::Red, Color::Red)
    } else if !warnings.is_empty() {
        (Color::Black, Color::Yellow, Color::Yellow)
    } else {
        (Color::White, Color::DarkGray, Color::Gray)
    };

    let title = if critical.len() > 1 {
        format!("⚠ {} CRITICAL ISSUES", critical.len())
    } else if !critical.is_empty() {
        "⚠ CRITICAL".to_string()
    } else {
        "⚠ Warnings".to_string()
    };

    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(fg).bg(bg))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(title)
                .border_style(Style::default().fg(border_color)),
        );
    f.render_widget(paragraph, area);
}

/// Draw the 3x3 grid of charts.
fn draw_charts(
    f: &mut Frame,
    metrics_history: &VecDeque<Metrics>,
    thresholds: &Thresholds,
    area: Rect,
) {
    if metrics_history.is_empty() {
        let loading = Paragraph::new("Waiting for data...").block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title("Charts"),
        );
        f.render_widget(loading, area);
        return;
    }

    // Get latest metrics for severity calculation
    let latest = metrics_history.back().unwrap();

    // 3 rows of charts (3x3 grid)
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(area);

    let row1 = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(rows[0]);

    let row2 = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(rows[1]);

    let row3 = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(rows[2]);

    // Row 1: I/O Read, I/O Write, CPU Usage
    draw_line_chart(
        f,
        metrics_history,
        row1[0],
        "I/O Read MB/s [bench]",
        |m| m.io_read_mb_per_sec.unwrap_or(0.0),
        ChartConfig {
            color: Color::Cyan,
            ..Default::default()
        },
    );

    draw_line_chart(
        f,
        metrics_history,
        row1[1],
        "I/O Write MB/s [bench]",
        |m| m.io_write_mb_per_sec.unwrap_or(0.0),
        ChartConfig {
            color: Color::LightCyan,
            ..Default::default()
        },
    );

    let cpu_severity = thresholds.cpu_usage_severity(latest.cpu_usage_percent);
    draw_line_chart(
        f,
        metrics_history,
        row1[2],
        "CPU % [/proc/stat]",
        |m| m.cpu_usage_percent as f64,
        ChartConfig {
            color: Color::Yellow,
            severity: cpu_severity,
            warning: Some(thresholds.cpu_usage_warning as f64),
            critical: Some(thresholds.cpu_usage_critical as f64),
        },
    );

    // Row 2: Memory Available, I/O Pressure, CPU Temp
    let mem_severity = thresholds.memory_available_severity(latest.mem_available_mb);
    draw_line_chart(
        f,
        metrics_history,
        row2[0],
        "Mem Avail MB [/proc/meminfo]",
        |m| m.mem_available_mb as f64,
        ChartConfig {
            color: Color::Green,
            severity: mem_severity,
            warning: Some(thresholds.memory_available_warning_mb as f64),
            critical: Some(thresholds.memory_available_critical_mb as f64),
        },
    );

    let io_pressure_severity =
        thresholds.io_pressure_severity(latest.io_pressure_some_avg10.unwrap_or(0.0));
    draw_line_chart(
        f,
        metrics_history,
        row2[1],
        "I/O Pressure % [PSI]",
        |m| m.io_pressure_some_avg10.unwrap_or(0.0),
        ChartConfig {
            color: Color::Magenta,
            severity: io_pressure_severity,
            warning: Some(thresholds.io_pressure_warning),
            critical: Some(thresholds.io_pressure_critical),
        },
    );

    let cpu_temp_severity = latest
        .cpu_temp_celsius
        .map(|t| thresholds.cpu_temp_severity(t))
        .unwrap_or(Severity::Normal);
    let cpu_temp_source = latest.cpu_temp_source.as_deref().unwrap_or("hwmon");
    let cpu_temp_title = format!("CPU °C [{}]", cpu_temp_source);
    draw_line_chart(
        f,
        metrics_history,
        row2[2],
        &cpu_temp_title,
        |m| m.cpu_temp_celsius.unwrap_or(0.0),
        ChartConfig {
            color: Color::LightYellow,
            severity: cpu_temp_severity,
            warning: Some(thresholds.cpu_temp_warning),
            critical: Some(thresholds.cpu_temp_critical),
        },
    );

    // Row 3: RAM Temp (DIMM), Disk Temp, IPMI Status
    let dimm_severity = latest
        .dimm_temp_max
        .map(|t| thresholds.dimm_temp_severity(t))
        .unwrap_or(Severity::Normal);
    let dimm_source = latest.dimm_temp_source.as_deref().unwrap_or("N/A");
    // Show DIMM names and source in title
    let dimm_title = if let Some(ref temps) = latest.dimm_temps {
        format!("RAM °C [{}] {}", dimm_source, temps)
    } else {
        format!("RAM °C [{}]", dimm_source)
    };
    draw_line_chart(
        f,
        metrics_history,
        row3[0],
        &dimm_title,
        |m| m.dimm_temp_max.unwrap_or(0.0),
        ChartConfig {
            color: Color::Red,
            severity: dimm_severity,
            warning: Some(thresholds.dimm_temp_warning),
            critical: Some(thresholds.dimm_temp_critical),
        },
    );

    let disk_severity = latest
        .disk_temp_max
        .map(|t| thresholds.disk_temp_severity(t))
        .unwrap_or(Severity::Normal);
    let disk_source = latest.disk_temp_source.as_deref().unwrap_or("N/A");
    // Show disk names and source in title
    let disk_title = if let Some(ref temps) = latest.disk_temps {
        format!("Disk °C [{}] {}", disk_source, temps)
    } else {
        format!("Disk °C [{}]", disk_source)
    };
    draw_line_chart(
        f,
        metrics_history,
        row3[1],
        &disk_title,
        |m| m.disk_temp_max.unwrap_or(0.0),
        ChartConfig {
            color: Color::LightRed,
            severity: disk_severity,
            warning: Some(thresholds.disk_temp_warning),
            critical: Some(thresholds.disk_temp_critical),
        },
    );

    // IPMI Temperature chart (shows all DIMM temps from BMC)
    draw_ipmi_temps_chart(f, metrics_history, thresholds, row3[2]);
}

/// Height for a bordered panel with up to `max_content_lines` visible body lines.
fn notification_panel_height(line_count: usize, max_content_lines: usize) -> u16 {
    let max_content_lines = max_content_lines.max(1);
    let visible_lines = line_count.clamp(1, max_content_lines);
    (visible_lines + 2) as u16
}

/// Join lines for a compact panel, replacing overflow with a summary line.
fn join_limited_lines(mut lines: Vec<String>, max_lines: usize) -> String {
    let max_lines = max_lines.max(1);
    if lines.len() > max_lines {
        let hidden = lines.len() - max_lines + 1;
        lines.truncate(max_lines - 1);
        lines.push(format!("... and {} more", hidden));
    }
    lines.join("\n")
}

/// Draw IPMI temperature chart showing all DIMM temperatures over time.
fn draw_ipmi_temps_chart(
    f: &mut Frame,
    metrics_history: &VecDeque<Metrics>,
    thresholds: &Thresholds,
    area: Rect,
) {
    // Check if we have any IPMI data
    let latest = match metrics_history.back() {
        Some(m) => m,
        None => return,
    };

    // If no IPMI data, show a placeholder
    if latest.ipmi_available != Some(true) || latest.ipmi_dimm_temps.is_empty() {
        let (text, color) = if latest.ipmi_available == Some(true) {
            ("No DIMM sensors found", Color::Gray)
        } else {
            ("N/A (need sudo + ipmitool)", Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("IPMI DIMM °C [ipmitool]")
            .border_style(Style::default().fg(color));

        let paragraph = Paragraph::new(text)
            .style(Style::default().fg(color))
            .block(block);

        f.render_widget(paragraph, area);
        return;
    }

    // Get unique DIMM names from latest reading
    let dimm_names: Vec<String> = latest
        .ipmi_dimm_temps
        .iter()
        .map(|d| d.name.clone())
        .collect();

    if dimm_names.is_empty() {
        return;
    }

    // Define colors for different DIMMs (cycle through these)
    let colors = [
        Color::Cyan,
        Color::Yellow,
        Color::Magenta,
        Color::Green,
        Color::LightBlue,
        Color::LightRed,
        Color::LightCyan,
        Color::LightMagenta,
    ];

    // Build data series for each DIMM
    let mut datasets_data: Vec<Vec<(f64, f64)>> = vec![Vec::new(); dimm_names.len()];

    for (time_idx, metrics) in metrics_history.iter().enumerate() {
        for (dimm_idx, dimm_name) in dimm_names.iter().enumerate() {
            // Find the temperature for this DIMM at this time
            let temp = metrics
                .ipmi_dimm_temps
                .iter()
                .find(|d| &d.name == dimm_name)
                .map(|d| d.temp_celsius)
                .unwrap_or(0.0);

            datasets_data[dimm_idx].push((time_idx as f64, temp));
        }
    }

    // Calculate Y-axis bounds
    let all_temps: Vec<f64> = datasets_data
        .iter()
        .flat_map(|d| d.iter().map(|(_, t)| *t))
        .collect();

    let min_y = all_temps
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min)
        .max(0.0);
    let max_y = all_temps.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    // Include thresholds in range calculation
    let warn_temp = thresholds.dimm_temp_warning;
    let crit_temp = thresholds.dimm_temp_critical;

    let range_max = max_y.max(warn_temp * 0.9);
    let y_range = if (range_max - min_y).abs() < 1.0 {
        (min_y - 5.0, range_max + 5.0)
    } else {
        (min_y * 0.95, range_max * 1.05)
    };

    // Determine severity based on max temperature
    let max_temp = latest.ipmi_dimm_temp_max.unwrap_or(0.0);
    let severity = thresholds.dimm_temp_severity(max_temp);

    let (border_color, title_style) = match severity {
        Severity::Critical => (
            Color::Red,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Severity::Warning => (
            Color::Yellow,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Severity::Normal => (Color::Reset, Style::default()),
    };

    // Build datasets for the chart
    let mut datasets: Vec<Dataset> = Vec::new();

    for (idx, (dimm_name, data)) in dimm_names.iter().zip(datasets_data.iter()).enumerate() {
        let color = colors[idx % colors.len()];

        // Shorten the name for the legend (e.g., "P1-DIMMA1" -> "A1")
        let short_name = shorten_dimm_name(dimm_name);

        // Get current temperature for this DIMM
        let current_temp = latest
            .ipmi_dimm_temps
            .iter()
            .find(|d| &d.name == dimm_name)
            .map(|d| d.temp_celsius)
            .unwrap_or(0.0);

        // Include current temp in legend: "A1:64"
        let legend_name = format!("{}:{:.0}", short_name, current_temp);

        // We need to keep the data alive, so use a reference
        datasets.push(
            Dataset::default()
                .name(legend_name)
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(color))
                .data(data),
        );
    }

    // Add warning threshold line if max temp is approaching it
    let data_len = metrics_history.len();
    let warning_line: Vec<(f64, f64)>;
    let critical_line: Vec<(f64, f64)>;

    if max_y >= warn_temp * 0.5 {
        warning_line = vec![(0.0, warn_temp), (data_len as f64, warn_temp)];
        datasets.push(
            Dataset::default()
                .name("warn")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(Color::Yellow))
                .data(&warning_line),
        );
    }

    if max_y >= crit_temp * 0.5 {
        critical_line = vec![(0.0, crit_temp), (data_len as f64, crit_temp)];
        datasets.push(
            Dataset::default()
                .name("crit")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(Color::Red))
                .data(&critical_line),
        );
    }

    // Build title with DIMM count, max temp and status
    let status_indicator = match latest.ipmi_dimm_status.as_deref() {
        Some("nr") => " NR!",
        Some("cr") => " CR!",
        Some("nc") => " NC",
        _ => "",
    };
    let dimm_count = dimm_names.len();
    let title = format!(
        "IPMI DIMM °C ({}) max:{:.0}{}",
        dimm_count, max_temp, status_indicator
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(title, title_style))
        .border_style(Style::default().fg(border_color));

    let chart = Chart::new(datasets)
        .block(block)
        .legend_position(Some(LegendPosition::TopRight))
        .hidden_legend_constraints((Constraint::Min(0), Constraint::Min(0)))
        .x_axis(
            Axis::default()
                .title("Time")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, data_len as f64]),
        )
        .y_axis(
            Axis::default()
                .title("")
                .style(Style::default().fg(Color::Gray))
                .labels(vec![
                    Span::raw(format!("{:.0}", y_range.0)),
                    Span::raw(format!("{:.0}", y_range.1)),
                ])
                .bounds([y_range.0, y_range.1]),
        );

    f.render_widget(chart, area);
}

/// Shorten DIMM name for chart legend (e.g., "DIMMA1 Temp." -> "A1", "P1-DIMMC1" -> "C1").
fn shorten_dimm_name(name: &str) -> String {
    // Remove common suffixes like "Temp.", "Temp", "Temperature"
    let name = name
        .trim()
        .trim_end_matches('.')
        .trim_end_matches(" Temp")
        .trim_end_matches(" Temperature")
        .trim_end_matches("Temp")
        .trim();

    let name_upper = name.to_uppercase();

    // Look for patterns like "DIMMA1", "DIMM A1", "P1-DIMMA1", etc.
    if let Some(pos) = name_upper.find("DIMM") {
        let suffix = &name[pos + 4..];
        let suffix = suffix.trim_start_matches(['-', '_', ' ']);
        if !suffix.is_empty() && suffix.len() <= 4 {
            return suffix.to_string();
        }
    }

    // Fallback: return last 4 chars or the whole name if shorter
    if name.len() <= 4 {
        name.to_string()
    } else {
        name[name.len() - 4..].to_string()
    }
}

/// Chart configuration including thresholds and styling.
#[derive(Default)]
struct ChartConfig {
    warning: Option<f64>,
    critical: Option<f64>,
    color: Color,
    severity: Severity,
}

/// Draw a single line chart with optional severity highlighting and threshold lines.
#[allow(clippy::too_many_arguments)]
fn draw_line_chart<F>(
    f: &mut Frame,
    metrics_history: &VecDeque<Metrics>,
    area: Rect,
    title: &str,
    value_fn: F,
    config: ChartConfig,
) where
    F: Fn(&Metrics) -> f64,
{
    let ChartConfig {
        warning,
        critical,
        color,
        severity,
    } = config;
    let data: Vec<(f64, f64)> = metrics_history
        .iter()
        .enumerate()
        .map(|(i, m)| (i as f64, value_fn(m)))
        .collect();

    if data.is_empty() {
        return;
    }

    let data_len = data.len();
    let min_y = data.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
    let max_y = data
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max);

    // Only show thresholds if data is within 50% of threshold value
    let show_warning = warning.map(|w| max_y >= w * 0.5).unwrap_or(false);
    let show_critical = critical.map(|c| max_y >= c * 0.5).unwrap_or(false);

    // Include thresholds in y-range calculation only if showing them
    let range_min = min_y;
    let mut range_max = max_y;
    if show_warning {
        if let Some(w) = warning {
            range_max = range_max.max(w * 1.1);
        }
    }
    if show_critical {
        if let Some(c) = critical {
            range_max = range_max.max(c * 1.1);
        }
    }

    let y_range = if (range_max - range_min).abs() < 0.001 {
        (range_min - 1.0, range_max + 1.0)
    } else {
        (range_min * 0.95, range_max * 1.05)
    };

    // Apply severity-based styling
    let (border_color, title_style) = match severity {
        Severity::Critical => (
            Color::Red,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Severity::Warning => (
            Color::Yellow,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Severity::Normal => (Color::Reset, Style::default()),
    };

    let mut datasets = vec![Dataset::default()
        .name(title)
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(color))
        .data(&data)];

    // Add warning threshold line (only if data is near threshold)
    let warning_line: Vec<(f64, f64)>;
    if show_warning {
        if let Some(w) = warning {
            warning_line = vec![(0.0, w), (data_len as f64, w)];
            datasets.push(
                Dataset::default()
                    .name("warn")
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Yellow))
                    .data(&warning_line),
            );
        }
    }

    // Add critical threshold line (only if data is near threshold)
    let critical_line: Vec<(f64, f64)>;
    if show_critical {
        if let Some(c) = critical {
            critical_line = vec![(0.0, c), (data_len as f64, c)];
            datasets.push(
                Dataset::default()
                    .name("crit")
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Red))
                    .data(&critical_line),
            );
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(title, title_style))
        .border_style(Style::default().fg(border_color));

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(
            Axis::default()
                .title("Time")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, data_len as f64]),
        )
        .y_axis({
            // Build Y axis labels including threshold values
            let mut labels = vec![Span::raw(format!("{:.0}", y_range.0))];
            if show_warning {
                if let Some(w) = warning {
                    labels.push(Span::styled(
                        format!("W:{:.0}", w),
                        Style::default().fg(Color::Yellow),
                    ));
                }
            }
            if show_critical {
                if let Some(c) = critical {
                    labels.push(Span::styled(
                        format!("C:{:.0}", c),
                        Style::default().fg(Color::Red),
                    ));
                }
            }
            labels.push(Span::raw(format!("{:.0}", y_range.1)));

            Axis::default()
                .title("")
                .style(Style::default().fg(Color::Gray))
                .labels(labels)
                .bounds([y_range.0, y_range.1])
        });

    f.render_widget(chart, area);
}

/// Draw the bottom detail panels.
fn draw_details(f: &mut Frame, metrics_history: &VecDeque<Metrics>, area: Rect) {
    let latest = match metrics_history.back() {
        Some(m) => m,
        None => return,
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);

    // Column 1: Benchmarks
    let bench_items = vec![
        ListItem::new(format!(
            "Read:   {:>7.1} MB/s",
            latest.io_read_mb_per_sec.unwrap_or(0.0)
        )),
        ListItem::new(format!(
            "Write:  {:>7.1} MB/s",
            latest.io_write_mb_per_sec.unwrap_or(0.0)
        )),
        ListItem::new(format!(
            "SHA256: {:>7.1} ms",
            latest.sha256_duration_ms.unwrap_or(0.0)
        )),
        ListItem::new(format!(
            "Alloc:  {:>7.1} ms",
            latest.memory_alloc_duration_ms
        )),
        ListItem::new(format!("Compute:{:>7.1} ms", latest.compute_duration_ms)),
    ];
    let bench_list = List::new(bench_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Benchmarks"),
    );
    f.render_widget(bench_list, cols[0]);

    // Column 2: Memory
    let mem_items = vec![
        ListItem::new(format!("Used:     {:>6} MB", latest.mem_used_mb)),
        ListItem::new(format!("Available:{:>6} MB", latest.mem_available_mb)),
        ListItem::new(format!("Buffers:  {:>6} MB", latest.mem_buffers_mb)),
        ListItem::new(format!("Cached:   {:>6} MB", latest.mem_cached_mb)),
        ListItem::new(format!("Swap:     {:>6} MB", latest.swap_used_mb)),
        ListItem::new(format!("Dirty:    {:>6} MB", latest.dirty_mb)),
    ];
    let mem_list = List::new(mem_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Memory"),
    );
    f.render_widget(mem_list, cols[1]);

    // Column 3: I/O & Pressure
    let io_items = vec![
        ListItem::new(format!(
            "IO Press: {:>5.1}%",
            latest.io_pressure_some_avg10.unwrap_or(0.0)
        )),
        ListItem::new(format!(
            "Mem Press:{:>5.1}%",
            latest.mem_pressure_some_avg10.unwrap_or(0.0)
        )),
        ListItem::new(format!(
            "CPU Press:{:>5.1}%",
            latest.cpu_pressure_some_avg10.unwrap_or(0.0)
        )),
        ListItem::new(format!("IOWait:   {:>6}", latest.cpu_iowait)),
        ListItem::new(format!("MajFaults:{:>6}", latest.pgmajfault)),
        ListItem::new(format!("SwapIn:   {:>6}", latest.pswpin)),
    ];
    let io_list = List::new(io_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Pressure/IO"),
    );
    f.render_widget(io_list, cols[2]);

    // Column 4: Temperatures & System
    let sys_items = vec![
        ListItem::new(format!(
            "CPU Temp: {:>5.1}C",
            latest.cpu_temp_celsius.unwrap_or(0.0)
        )),
        ListItem::new(format!(
            "RAM Temp: {:>5.1}C",
            latest.dimm_temp_max.unwrap_or(0.0)
        )),
        ListItem::new(format!(
            "Disk Temp:{:>5.1}C",
            latest.disk_temp_max.unwrap_or(0.0)
        )),
        ListItem::new(format!("Procs:    {:>6}", latest.process_count)),
        ListItem::new(format!("Blocked:  {:>6}", latest.procs_blocked)),
        ListItem::new(format!("FDs:      {:>6}", latest.fd_allocated)),
    ];
    let sys_list = List::new(sys_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Temps/Sys"),
    );
    f.render_widget(sys_list, cols[3]);
}

/// Run in headless mode (no TUI, just logging to stdout).
///
/// # Arguments
///
/// * `app` - Application instance
/// * `running` - Atomic flag to signal shutdown
/// * `interval` - Time between metric collections
pub fn run_headless(
    mut app: App,
    running: Arc<AtomicBool>,
    interval: Duration,
) -> std::io::Result<()> {
    let csv_file = app.config.csv_file.clone();
    let history_size = app.config.history_size;

    println!("slow-rs - System Slowness Diagnostic Monitor");
    println!("=============================================");
    println!("Logging to: {}", csv_file);
    println!("Interval: {} seconds", interval.as_secs());
    println!("Press Ctrl+C to stop.\n");

    while running.load(Ordering::Relaxed) {
        let metrics = app.collect_metrics()?;

        // Print summary line
        println!(
            "[{}] CPU: {:5.1}% | Mem: {:6}/{:6} MB | Load: {:5.2} {:5.2} {:5.2} | Read: {:7.1} MB/s | IOPress: {:5.1}%",
            metrics.datetime,
            metrics.cpu_usage_percent,
            metrics.mem_used_mb,
            metrics.mem_total_mb,
            metrics.load_avg_1,
            metrics.load_avg_5,
            metrics.load_avg_15,
            metrics.io_read_mb_per_sec.unwrap_or(0.0),
            metrics.io_pressure_some_avg10.unwrap_or(0.0),
        );

        add_metrics(&mut app.metrics_history, metrics, history_size);
        std::thread::sleep(interval);
    }

    println!("\nStopped. Data logged to {}", csv_file);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{join_limited_lines, notification_panel_height, shorten_dimm_name};

    #[test]
    fn notification_panel_height_reserves_borders_and_caps_body_lines() {
        assert_eq!(notification_panel_height(1, 3), 3);
        assert_eq!(notification_panel_height(3, 3), 5);
        assert_eq!(notification_panel_height(8, 3), 5);
    }

    #[test]
    fn join_limited_lines_summarizes_overflow() {
        let lines = vec![
            "one".to_string(),
            "two".to_string(),
            "three".to_string(),
            "four".to_string(),
        ];

        assert_eq!(
            join_limited_lines(lines, 3),
            "one\ntwo\n... and 2 more".to_string()
        );
    }

    #[test]
    fn shorten_dimm_name_keeps_slot_identifier() {
        assert_eq!(shorten_dimm_name("P1-DIMMA1"), "A1");
        assert_eq!(shorten_dimm_name("DIMMC1 Temp."), "C1");
    }
}
