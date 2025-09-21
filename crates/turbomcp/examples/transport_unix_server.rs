//! Unix Socket Transport Server - Local IPC
//!
//! This example demonstrates the Unix domain socket transport which
//! provides efficient local inter-process communication.
//!
//! Run with: `cargo run --example transport_unix_server`

use std::sync::Arc;
use tokio::sync::Mutex;
use turbomcp::prelude::*;

/// Process manager using Unix socket transport (macro approach)
#[derive(Clone)]
struct ProcessManager {
    processes: Arc<Mutex<Vec<ProcessInfo>>>,
}

#[derive(Clone, Debug)]
struct ProcessInfo {
    id: u32,
    name: String,
    status: String,
    cpu_usage: f64,
    memory_mb: u32,
}

#[server(
    name = "Process Manager",
    version = "1.0.0",
    description = "System process management via Unix sockets"
)]
impl ProcessManager {
    fn new() -> Self {
        // Simulate some system processes
        let processes = vec![
            ProcessInfo {
                id: 1001,
                name: "turbomcp-server".to_string(),
                status: "running".to_string(),
                cpu_usage: 2.3,
                memory_mb: 45,
            },
            ProcessInfo {
                id: 1002,
                name: "mcp-client".to_string(),
                status: "running".to_string(),
                cpu_usage: 1.8,
                memory_mb: 32,
            },
            ProcessInfo {
                id: 1003,
                name: "system-monitor".to_string(),
                status: "sleeping".to_string(),
                cpu_usage: 0.1,
                memory_mb: 12,
            },
        ];

        Self {
            processes: Arc::new(Mutex::new(processes)),
        }
    }

    #[tool("List all processes")]
    async fn list_processes(&self) -> McpResult<String> {
        let processes = self.processes.lock().await;
        let mut output = String::from("🔄 System Processes:\n");

        for process in processes.iter() {
            output.push_str(&format!(
                "├─ PID {}: {} [{}] - CPU: {:.1}%, RAM: {}MB\n",
                process.id, process.name, process.status, process.cpu_usage, process.memory_mb
            ));
        }

        output.push_str(&format!("└─ Total processes: {}", processes.len()));
        Ok(output)
    }

    #[tool("Start a new process")]
    async fn start_process(&self, name: String) -> McpResult<String> {
        let mut processes = self.processes.lock().await;
        let new_id = processes.iter().map(|p| p.id).max().unwrap_or(1000) + 1;

        let new_process = ProcessInfo {
            id: new_id,
            name: name.clone(),
            status: "starting".to_string(),
            cpu_usage: fastrand::f64() * 5.0,
            memory_mb: fastrand::u32(10..100),
        };

        processes.push(new_process);
        Ok(format!("🚀 Started process '{}' with PID {}", name, new_id))
    }

    #[tool("Stop a process")]
    async fn stop_process(&self, pid: u32) -> McpResult<String> {
        let mut processes = self.processes.lock().await;

        if let Some(process) = processes.iter_mut().find(|p| p.id == pid) {
            process.status = "stopped".to_string();
            process.cpu_usage = 0.0;
            Ok(format!(
                "🛑 Stopped process '{}' (PID {})",
                process.name, pid
            ))
        } else {
            Err(McpError::tool(format!(
                "Process with PID {} not found",
                pid
            )))
        }
    }

    #[tool("Get process details")]
    async fn get_process(&self, pid: u32) -> McpResult<String> {
        let processes = self.processes.lock().await;

        if let Some(process) = processes.iter().find(|p| p.id == pid) {
            Ok(format!(
                "📋 Process Details:\n\
                 🆔 PID: {}\n\
                 📛 Name: {}\n\
                 ⚡ Status: {}\n\
                 🖥️  CPU Usage: {:.1}%\n\
                 💾 Memory: {}MB\n\
                 🔌 Transport: Unix Socket",
                process.id, process.name, process.status, process.cpu_usage, process.memory_mb
            ))
        } else {
            Err(McpError::tool(format!(
                "Process with PID {} not found",
                pid
            )))
        }
    }

    #[tool("Get system statistics")]
    async fn get_system_stats(&self) -> McpResult<String> {
        let processes = self.processes.lock().await;
        let total_processes = processes.len();
        let running_processes = processes.iter().filter(|p| p.status == "running").count();
        let total_cpu: f64 = processes.iter().map(|p| p.cpu_usage).sum();
        let total_memory: u32 = processes.iter().map(|p| p.memory_mb).sum();

        Ok(format!(
            "📊 System Statistics:\n\
             🔄 Total processes: {}\n\
             ✅ Running processes: {}\n\
             🖥️  Total CPU usage: {:.1}%\n\
             💾 Total memory usage: {}MB\n\
             🔌 IPC method: Unix Domain Sockets",
            total_processes, running_processes, total_cpu, total_memory
        ))
    }

    #[resource("unix:///proc/status")]
    async fn proc_status(&self, _ctx: Context) -> McpResult<String> {
        let processes = self.processes.lock().await;
        let running = processes.iter().filter(|p| p.status == "running").count();
        let stopped = processes.iter().filter(|p| p.status == "stopped").count();
        let sleeping = processes.iter().filter(|p| p.status == "sleeping").count();

        Ok(format!(
            "📊 Process Status Summary:\n\
             🟢 Running: {}\n\
             🔴 Stopped: {}\n\
             😴 Sleeping: {}\n\
             📡 IPC: Unix Socket (Local)\n\
             ⚡ Performance: High (zero-copy)",
            running, stopped, sleeping
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: For MCP STDIO protocol, logs MUST go to stderr, not stdout
    // stdout is reserved for pure JSON-RPC messages only
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr) // Fix: Send logs to stderr
        .init();

    tracing::info!("🔄 Starting Process Manager (Unix Socket Transport)");
    tracing::info!("Unix socket will be available at: /tmp/turbomcp-process.sock");
    tracing::info!("Features: Local IPC, high performance, system integration");

    let manager = ProcessManager::new();

    // Unix socket transport - local IPC
    manager.run_unix("/tmp/turbomcp-process.sock").await?;

    Ok(())
}
