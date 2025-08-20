use std::collections::VecDeque;
use std::io;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table},
};

use crate::{cpu, iokit, ioreport_perf, memory, time_graph::TimeGraph};

enum MetricEvent {
    Update(MetricData),
}

struct MetricData {
    memory: memory::MemoryMetrics,
    power: Option<iokit::PowerMetrics>,
    performance: Option<ioreport_perf::PerformanceSample>,
}

const MAX_HISTORY: usize = 128;

struct DashboardState {
    // CPU info (static, doesn't change)
    cpu_metrics: Option<cpu::CpuMetrics>,

    // Current values
    current_memory: Option<memory::MemoryMetrics>,
    current_power: Option<iokit::PowerMetrics>,
    current_performance: Option<ioreport_perf::PerformanceSample>,

    // Historical data for sparklines
    memory_history: VecDeque<u64>,      // RAM usage in bytes
    cpu_power_history: VecDeque<u64>,   // CPU power in milliwatts
    gpu_power_history: VecDeque<u64>,   // GPU power in milliwatts
    ane_power_history: VecDeque<u64>,   // ANE power in milliwatts
    total_power_history: VecDeque<u64>, // Total power in milliwatts

    ecpu_freq_history: VecDeque<u64>, // E-CPU frequency in MHz
    pcpu_freq_history: VecDeque<u64>, // P-CPU frequency in MHz
    gpu_freq_history: VecDeque<u64>,  // GPU frequency in MHz

    ecpu_usage_history: VecDeque<u64>, // E-CPU usage 0-100
    pcpu_usage_history: VecDeque<u64>, // P-CPU usage 0-100
    gpu_usage_history: VecDeque<u64>,  // GPU usage 0-100
    cpu_usage_history: VecDeque<u64>,  // Combined CPU usage 0-100
}

impl DashboardState {
    fn new() -> Self {
        Self {
            cpu_metrics: None,
            current_memory: None,
            current_power: None,
            current_performance: None,
            memory_history: VecDeque::with_capacity(MAX_HISTORY),
            cpu_power_history: VecDeque::with_capacity(MAX_HISTORY),
            gpu_power_history: VecDeque::with_capacity(MAX_HISTORY),
            ane_power_history: VecDeque::with_capacity(MAX_HISTORY),
            total_power_history: VecDeque::with_capacity(MAX_HISTORY),
            ecpu_freq_history: VecDeque::with_capacity(MAX_HISTORY),
            pcpu_freq_history: VecDeque::with_capacity(MAX_HISTORY),
            gpu_freq_history: VecDeque::with_capacity(MAX_HISTORY),
            ecpu_usage_history: VecDeque::with_capacity(MAX_HISTORY),
            pcpu_usage_history: VecDeque::with_capacity(MAX_HISTORY),
            gpu_usage_history: VecDeque::with_capacity(MAX_HISTORY),
            cpu_usage_history: VecDeque::with_capacity(MAX_HISTORY),
        }
    }

    fn update(&mut self, data: MetricData) {
        // Update current values
        self.current_memory = Some(data.memory.clone());
        self.current_power = data.power.clone();
        self.current_performance = data.performance.clone();

        // Update memory history
        self.memory_history.push_front(data.memory.ram_usage);
        if self.memory_history.len() > MAX_HISTORY {
            self.memory_history.pop_back();
        }

        // Update power history
        if let Some(ref power) = data.power {
            self.cpu_power_history
                .push_front((power.cpu_power * 1000.0) as u64);
            self.gpu_power_history
                .push_front((power.gpu_power * 1000.0) as u64);
            self.ane_power_history
                .push_front((power.ane_power * 1000.0) as u64);
            self.total_power_history
                .push_front((power.all_power * 1000.0) as u64);

            if self.cpu_power_history.len() > MAX_HISTORY {
                self.cpu_power_history.pop_back();
                self.gpu_power_history.pop_back();
                self.ane_power_history.pop_back();
                self.total_power_history.pop_back();
            }
        }

        // Update performance history
        if let Some(ref perf) = data.performance {
            self.ecpu_freq_history.push_front(perf.ecpu_usage.0 as u64);
            self.pcpu_freq_history.push_front(perf.pcpu_usage.0 as u64);
            self.gpu_freq_history.push_front(perf.gpu_usage.0 as u64);

            self.ecpu_usage_history
                .push_front((perf.ecpu_usage.1 * 100.0) as u64);
            self.pcpu_usage_history
                .push_front((perf.pcpu_usage.1 * 100.0) as u64);
            self.gpu_usage_history
                .push_front((perf.gpu_usage.1 * 100.0) as u64);

            // Calculate combined CPU usage (weighted average of E and P cores)
            let combined_cpu = ((perf.ecpu_usage.1 + perf.pcpu_usage.1) / 2.0 * 100.0) as u64;
            self.cpu_usage_history.push_front(combined_cpu);

            if self.ecpu_freq_history.len() > MAX_HISTORY {
                self.ecpu_freq_history.pop_back();
                self.pcpu_freq_history.pop_back();
                self.gpu_freq_history.pop_back();
                self.ecpu_usage_history.pop_back();
                self.pcpu_usage_history.pop_back();
                self.gpu_usage_history.pop_back();
                self.cpu_usage_history.pop_back();
            }
        }
    }
}

pub struct Dashboard {
    refresh_interval: Duration,
    state: DashboardState,
    metric_receiver: Receiver<MetricEvent>,
}

impl Dashboard {
    pub fn new() -> io::Result<Self> {
        let (tx, rx) = mpsc::channel::<MetricEvent>();
        let refresh_interval = Duration::from_millis(1000);

        // Spawn metric collection thread that runs continuously
        let tx_clone = tx;
        let interval = refresh_interval.clone();
        thread::spawn(move || {
            let perf_monitor = ioreport_perf::IOReportPerf::new().ok();

            loop {
                // Collect all metrics in one go
                let memory = memory::get_memory_metrics().ok();
                let power =
                    iokit::get_power_metrics_with_interval(interval.as_millis() as u64).ok();
                let performance = perf_monitor
                    .as_ref()
                    .map(|m| m.get_sample(interval.as_millis() as u64));

                if let Some(mem) = memory {
                    let _ = tx_clone.send(MetricEvent::Update(MetricData {
                        memory: mem,
                        power,
                        performance,
                    }));
                }

                // Sleep for the interval duration
                thread::sleep(interval);
            }
        });

        Ok(Dashboard {
            refresh_interval,
            state: DashboardState::new(),
            metric_receiver: rx,
        })
    }

    pub fn run(&mut self) -> io::Result<()> {
        // ==============================================================================
        // Terminal Setup
        // ==============================================================================
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        crossterm::execute!(
            stdout,
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;

        let backend = ratatui::backend::CrosstermBackend::new(stdout);
        let mut terminal = ratatui::Terminal::new(backend)?;

        // Get CPU metrics once (they don't change)
        self.state.cpu_metrics = Some(
            cpu::get_cpu_metrics()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{}", e)))?,
        );

        // ==============================================================================
        // Main Event Loop
        // ==============================================================================
        let result = self.event_loop(&mut terminal);

        // ==============================================================================
        // Cleanup
        // ==============================================================================
        crossterm::terminal::disable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
    }

    fn event_loop(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    ) -> io::Result<()> {
        loop {
            // Draw the dashboard
            terminal.draw(|f| self.render(f))?;

            // Process all pending metrics from the collection thread
            while let Ok(MetricEvent::Update(data)) = self.metric_receiver.try_recv() {
                self.state.update(data);
            }

            // Poll for keyboard events with a timeout
            // This timeout controls the UI refresh rate when no events occur
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                // Increase refresh interval (slower refresh)
                                let millis = self.refresh_interval.as_millis() as u64;
                                if millis < 5000 {
                                    self.refresh_interval = Duration::from_millis(millis + 100);
                                    // TODO: Signal the metric thread to update interval
                                }
                            }
                            KeyCode::Char('-') => {
                                // Decrease refresh interval (faster refresh)
                                let millis = self.refresh_interval.as_millis() as u64;
                                if millis > 100 {
                                    self.refresh_interval = Duration::from_millis(millis - 100);
                                    // TODO: Signal the metric thread to update interval
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(0),    // Content
                Constraint::Length(3), // Footer
            ])
            .split(frame.area());

        // ==============================================================================
        // Header
        // ==============================================================================
        let header = Paragraph::new(vec![Line::from(vec![
            Span::styled(
                "atop",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" - macOS System Monitor"),
        ])])
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);
        frame.render_widget(header, chunks[0]);

        // ==============================================================================
        // Main Content Area
        // ==============================================================================
        let content_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7), // CPU info text
                Constraint::Length(8), // CPU Usage Graph
                Constraint::Length(8), // Memory Graph
                Constraint::Length(8), // Frequency Graphs
                Constraint::Length(8), // Power Graphs
                Constraint::Min(8),    // Performance table
            ])
            .split(chunks[1]);

        // CPU info text
        self.render_cpu_info(frame, content_chunks[0]);

        // CPU Usage Graph
        self.render_cpu_graph(frame, content_chunks[1]);

        // Memory Graph
        self.render_memory_info(frame, content_chunks[2]);

        // Frequency Graphs
        self.render_frequency_graphs(frame, content_chunks[3]);

        // Power Graphs
        self.render_power_info(frame, content_chunks[4]);

        // Performance Table
        self.render_performance_table(frame, content_chunks[5]);

        // ==============================================================================
        // Footer with Controls
        // ==============================================================================
        let footer_text = format!(
            "Refresh: {:.1}s | [+/-] Adjust Rate | [q/ESC] Quit",
            self.refresh_interval.as_secs_f32()
        );
        let footer = Paragraph::new(footer_text)
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(footer, chunks[2]);
    }

    fn render_cpu_info(&self, frame: &mut Frame, area: Rect) {
        let cpu_info = if let Some(ref cpu) = self.state.cpu_metrics {
            vec![
                Line::from(vec![
                    Span::raw("Brand: "),
                    Span::styled(&cpu.cpu_brand, Style::default().fg(Color::Yellow)),
                ]),
                Line::from(vec![
                    Span::raw("Cores: "),
                    Span::styled(
                        format!(
                            "{} physical, {} logical",
                            cpu.physical_cores, cpu.logical_cores
                        ),
                        Style::default().fg(Color::Green),
                    ),
                ]),
                if let (Some(p), Some(e)) = (cpu.pcpu_cores, cpu.ecpu_cores) {
                    Line::from(vec![
                        Span::raw("  P/E: "),
                        Span::styled(
                            format!("{} P-cores, {} E-cores", p, e),
                            Style::default().fg(Color::Cyan),
                        ),
                    ])
                } else {
                    Line::from(vec![])
                },
                Line::from(vec![
                    Span::raw("Freq: "),
                    Span::styled(
                        format!("{} MHz", cpu.cpu_frequency_mhz),
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
            ]
        } else {
            vec![Line::from("Loading...")]
        };

        let cpu_block = Paragraph::new(cpu_info).block(
            Block::default()
                .title(" CPU ")
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::White)),
        );
        frame.render_widget(cpu_block, area);
    }

    fn render_memory_info(&self, frame: &mut Frame, area: Rect) {
        if let Some(ref mem) = self.state.current_memory {
            let total_gb = mem.ram_total as f64 / 1_073_741_824.0;
            let used_gb = mem.ram_usage as f64 / 1_073_741_824.0;
            let usage_percent = (mem.ram_usage as f64 / mem.ram_total as f64 * 100.0) as u64;

            let graph = TimeGraph::new(&self.state.memory_history)
                .max(mem.ram_total)
                .style(Style::default().fg(Color::Blue))
                .block(
                    Block::default()
                        .title(format!(
                            " Memory: {:.1}/{:.1} GB ({}%) ",
                            used_gb, total_gb, usage_percent
                        ))
                        .borders(Borders::ALL),
                );

            frame.render_widget(graph, area);
        } else {
            let loading = Paragraph::new("Loading...")
                .block(Block::default().title(" Memory ").borders(Borders::ALL));
            frame.render_widget(loading, area);
        }
    }

    fn render_power_info(&self, frame: &mut Frame, area: Rect) {
        // Split area into 4 sections: Total, CPU, GPU, ANE power
        let power_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(area);

        if let Some(ref power) = self.state.current_power {
            // Total Power Graph
            let max_power = 50000; // 50W max for display
            let total_graph = TimeGraph::new(&self.state.total_power_history)
                .max(max_power as u64)
                .style(Style::default().fg(Color::White))
                .block(
                    Block::default()
                        .title(format!(" Total: {:.2}W ", power.all_power))
                        .borders(Borders::ALL),
                );
            frame.render_widget(total_graph, power_chunks[0]);

            // CPU Power Graph
            let cpu_graph = TimeGraph::new(&self.state.cpu_power_history)
                .max(max_power as u64)
                .style(Style::default().fg(Color::Red))
                .block(
                    Block::default()
                        .title(format!(" CPU: {:.2}W ", power.cpu_power))
                        .borders(Borders::ALL),
                );
            frame.render_widget(cpu_graph, power_chunks[1]);

            // GPU Power Graph
            let gpu_graph = TimeGraph::new(&self.state.gpu_power_history)
                .max(max_power as u64)
                .style(Style::default().fg(Color::Magenta))
                .block(
                    Block::default()
                        .title(format!(" GPU: {:.2}W ", power.gpu_power))
                        .borders(Borders::ALL),
                );
            frame.render_widget(gpu_graph, power_chunks[2]);

            // ANE Power Graph
            let ane_graph = TimeGraph::new(&self.state.ane_power_history)
                .max(max_power as u64)
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .title(format!(" ANE: {:.2}W ", power.ane_power))
                        .borders(Borders::ALL),
                );
            frame.render_widget(ane_graph, power_chunks[3]);
        } else {
            let no_data = Paragraph::new("Power metrics not available")
                .block(Block::default().title(" Power ").borders(Borders::ALL));
            frame.render_widget(no_data, area);
        }
    }

    fn render_performance_table(&self, frame: &mut Frame, area: Rect) {
        if let Some(ref perf) = self.state.current_performance {
            let header = Row::new(vec!["Cluster", "Frequency", "Utilization"])
                .style(Style::default().add_modifier(Modifier::BOLD))
                .bottom_margin(1);

            let ecpu_freq = format!("{} MHz", perf.ecpu_usage.0);
            let ecpu_util = format!("{:.1}%", perf.ecpu_usage.1);
            let pcpu_freq = format!("{} MHz", perf.pcpu_usage.0);
            let pcpu_util = format!("{:.1}%", perf.pcpu_usage.1);
            let gpu_freq = format!("{} MHz", perf.gpu_usage.0);
            let gpu_util = format!("{:.1}%", perf.gpu_usage.1);

            let rows = vec![
                Row::new(vec!["E-Cluster", &ecpu_freq, &ecpu_util]),
                Row::new(vec!["P-Cluster", &pcpu_freq, &pcpu_util]),
                Row::new(vec!["GPU", &gpu_freq, &gpu_util]),
            ];

            let table = Table::new(
                rows,
                [
                    Constraint::Length(10),
                    Constraint::Length(12),
                    Constraint::Length(12),
                ],
            )
            .header(header)
            .block(
                Block::default()
                    .title(" Performance ")
                    .borders(Borders::ALL),
            );

            frame.render_widget(table, area);
        } else {
            let no_data = Paragraph::new("Performance metrics not available").block(
                Block::default()
                    .title(" Performance ")
                    .borders(Borders::ALL),
            );
            frame.render_widget(no_data, area);
        }
    }

    fn render_cpu_graph(&self, frame: &mut Frame, area: Rect) {
        let current_usage = if let Some(ref perf) = self.state.current_performance {
            ((perf.ecpu_usage.1 + perf.pcpu_usage.1) / 2.0 * 100.0) as u64
        } else {
            0
        };

        let graph = TimeGraph::new(&self.state.cpu_usage_history)
            .max(100)
            .style(Style::default().fg(Color::Cyan))
            .block(
                Block::default()
                    .title(format!(" CPU Usage: {}% ", current_usage))
                    .borders(Borders::ALL),
            );

        frame.render_widget(graph, area);
    }

    fn render_frequency_graphs(&self, frame: &mut Frame, area: Rect) {
        // Split into 3 sections for E-CPU, P-CPU, GPU frequencies
        let freq_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(34),
            ])
            .split(area);

        if let Some(ref perf) = self.state.current_performance {
            // E-CPU Frequency Graph
            let ecpu_graph = TimeGraph::new(&self.state.ecpu_freq_history)
                .max(4000) // 4000 MHz max
                .style(Style::default().fg(Color::Green))
                .block(
                    Block::default()
                        .title(format!(
                            " E-CPU: {} MHz ({:.0}%) ",
                            perf.ecpu_usage.0,
                            perf.ecpu_usage.1 * 100.0
                        ))
                        .borders(Borders::ALL),
                );
            frame.render_widget(ecpu_graph, freq_chunks[0]);

            // P-CPU Frequency Graph
            let pcpu_graph = TimeGraph::new(&self.state.pcpu_freq_history)
                .max(4000) // 4000 MHz max
                .style(Style::default().fg(Color::Cyan))
                .block(
                    Block::default()
                        .title(format!(
                            " P-CPU: {} MHz ({:.0}%) ",
                            perf.pcpu_usage.0,
                            perf.pcpu_usage.1 * 100.0
                        ))
                        .borders(Borders::ALL),
                );
            frame.render_widget(pcpu_graph, freq_chunks[1]);

            // GPU Frequency Graph
            let gpu_graph = TimeGraph::new(&self.state.gpu_freq_history)
                .max(2000) // 2000 MHz max for GPU
                .style(Style::default().fg(Color::Magenta))
                .block(
                    Block::default()
                        .title(format!(
                            " GPU: {} MHz ({:.0}%) ",
                            perf.gpu_usage.0,
                            perf.gpu_usage.1 * 100.0
                        ))
                        .borders(Borders::ALL),
                );
            frame.render_widget(gpu_graph, freq_chunks[2]);
        } else {
            let no_data = Paragraph::new("Frequency data not available").block(
                Block::default()
                    .title(" Frequencies ")
                    .borders(Borders::ALL),
            );
            frame.render_widget(no_data, area);
        }
    }
}
