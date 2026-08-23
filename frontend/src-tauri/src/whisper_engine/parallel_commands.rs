use tauri::State;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;

use crate::whisper_engine::{ParallelProcessor, ParallelConfig, SystemMonitor};

// Global state for parallel processor
pub struct ParallelProcessorState {
    pub processor: Arc<RwLock<Option<ParallelProcessor>>>,
    pub system_monitor: Arc<SystemMonitor>,
}

impl Default for ParallelProcessorState {
    fn default() -> Self {
        Self::new()
    }
}

impl ParallelProcessorState {
    pub fn new() -> Self {
        Self {
            processor: Arc::new(RwLock::new(None)),
            system_monitor: Arc::new(SystemMonitor::new()),
        }
    }
}

#[tauri::command]
pub async fn initialize_parallel_processor(
    state: State<'_, ParallelProcessorState>,
    max_workers: Option<usize>,
    memory_budget_mb: Option<u64>,
) -> Result<String, String> {
    let mut config = ParallelConfig::default();

    if let Some(workers) = max_workers {
        config.max_workers = std::cmp::min(workers, 4); // Safety limit
    }

    if let Some(memory) = memory_budget_mb {
        config.memory_budget_mb = memory;
    }

    // Calculate safe worker count based on system resources
    let safe_workers = state.system_monitor
        .calculate_safe_worker_count()
        .await
        .map_err(|e| format!("Failed to calculate safe worker count: {}", e))?;

    config.max_workers = std::cmp::min(config.max_workers, safe_workers);

    let (processor, _event_receiver) = ParallelProcessor::new(
        config.clone(),
        state.system_monitor.clone()
    ).map_err(|e| format!("Failed to create parallel processor: {}", e))?;

    *state.processor.write().await = Some(processor);

    Ok(format!("Parallel processor initialized with {} workers, {}MB memory per worker",
               config.max_workers, config.memory_budget_mb))
}

pub async fn check_resource_constraints(
    state: State<'_, ParallelProcessorState>,
) -> Result<serde_json::Value, String> {
    let status = state.system_monitor.check_resource_constraints()
        .await
        .map_err(|e| format!("Failed to check resource constraints: {}", e))?;

    serde_json::to_value(status)
        .map_err(|e| format!("Failed to serialize resource status: {}", e))
}
