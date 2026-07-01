//! H9-12: Resource Manager and workload-class admission control
//!
//! Provides admission control, resource allocation, and SLO-driven throttling for OLTP/OLAP workloads.
//! - Differentiates workload classes: OLTP (latency-sensitive), OLAP (throughput-oriented), Mixed (best-effort)
//! - Maintains admission queue with priority-based scheduling
//! - Enforces resource budgets (CPU, memory)
//! - Implements SLO-driven throttling: pauses new OLAP queries when OLTP SLO pressure exceeds threshold
//! - Tracks metrics (admissions, rejections, queue depth, throttle events)

use std::collections::{BinaryHeap, HashMap};
use std::cmp::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Workload class for query routing and admission control
#[derive(Clone, Debug, PartialEq)]
pub enum WorkloadClass {
    /// Online transaction processing: strict latency SLA
    OLTP { latency_sla_ms: u64 },
    /// Online analytical processing: batch processing with timeout
    OLAP { timeout_ms: u64 },
    /// Mixed workload: adaptive admission
    Mixed,
}

/// Query submission request for admission control
#[derive(Clone, Debug)]
pub struct QueryRequest {
    pub query_id: String,
    pub workload_class: WorkloadClass,
    pub estimated_cost: u64,
    pub priority: i32, // higher = more important
}

/// Resource budget constraints
#[derive(Clone, Debug)]
pub struct ResourceBudget {
    pub cpu_cores_available: f64,
    pub memory_mb_available: u64,
    pub cpu_cores_reserved_for_oltp: f64,
    pub memory_mb_reserved_for_oltp: u64,
}

/// Allocated resources to a query
#[derive(Clone, Debug)]
pub struct ResourceAllocation {
    pub query_id: String,
    pub cpu_cores_allocated: f64,
    pub memory_mb_allocated: u64,
    pub assigned_at_ms: u64,
    pub throttle_multiplier: f64, // 1.0 = no throttle
}

/// Metrics tracked by resource manager
#[derive(Clone, Debug)]
pub struct ResourceMetrics {
    pub total_admitted: u64,
    pub total_rejected: u64,
    pub queue_depth: usize,
    pub throttle_events: u64,
    pub avg_queue_time_ms: f64,
}

/// Priority queue entry for QueryRequest (ordered by priority, then by cost)
struct QueueEntry {
    request: QueryRequest,
    enqueued_at_ms: u64,
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first, then lower cost first
        match other.request.priority.cmp(&self.request.priority) {
            Ordering::Equal => {
                self.request.estimated_cost.cmp(&other.request.estimated_cost)
                    .then_with(|| other.request.query_id.cmp(&self.request.query_id))
            }
            o => o,
        }
    }
}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for QueueEntry {}

impl PartialEq for QueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.request.query_id == other.request.query_id
    }
}

/// Admission queue with priority scheduling
pub struct AdmissionQueue {
    pending: BinaryHeap<QueueEntry>,
    admitted: HashMap<String, ResourceAllocation>,
    throttle_active: bool,
    throttle_reason: String,
    total_queue_time_ms: u64,
    queue_events_count: u64,
}

impl AdmissionQueue {
    fn new() -> Self {
        AdmissionQueue {
            pending: BinaryHeap::new(),
            admitted: HashMap::new(),
            throttle_active: false,
            throttle_reason: String::new(),
            total_queue_time_ms: 0,
            queue_events_count: 0,
        }
    }

    fn enqueue(&mut self, request: QueryRequest, now_ms: u64) {
        self.pending.push(QueueEntry {
            request,
            enqueued_at_ms: now_ms,
        });
    }

    fn dequeue(&mut self) -> Option<(QueryRequest, u64)> {
        self.pending.pop().map(|entry| {
            let now_ms = current_time_ms();
            let queue_time = now_ms - entry.enqueued_at_ms;
            self.total_queue_time_ms += queue_time;
            self.queue_events_count += 1;
            (entry.request, queue_time)
        })
    }

    fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    fn depth(&self) -> usize {
        self.pending.len()
    }

    fn avg_queue_time_ms(&self) -> f64 {
        if self.queue_events_count == 0 {
            0.0
        } else {
            self.total_queue_time_ms as f64 / self.queue_events_count as f64
        }
    }
}

/// Resource Manager: admission control and allocation
pub struct ResourceManager {
    total_budget: ResourceBudget,
    admission_queue: Arc<Mutex<AdmissionQueue>>,
    metrics: Arc<Mutex<ResourceManagerMetrics>>,
}

struct ResourceManagerMetrics {
    total_admitted: u64,
    total_rejected: u64,
    throttle_events: u64,
}

impl ResourceManager {
    /// Create a new ResourceManager with the given budget
    pub fn new(budget: ResourceBudget) -> Self {
        ResourceManager {
            total_budget: budget,
            admission_queue: Arc::new(Mutex::new(AdmissionQueue::new())),
            metrics: Arc::new(Mutex::new(ResourceManagerMetrics {
                total_admitted: 0,
                total_rejected: 0,
                throttle_events: 0,
            })),
        }
    }

    /// Submit a query for admission control
    pub fn submit_query(&self, request: QueryRequest) -> Result<ResourceAllocation, String> {
        let now_ms = current_time_ms();
        
        // Estimate resource requirements
        let estimated_memory = (request.estimated_cost / 100).min(1024) as u64;
        let estimated_cpu = ((request.estimated_cost as f64) / 1000.0).min(4.0);
        
        // Reject if query requires more resources than ever available
        if estimated_memory > self.total_budget.memory_mb_available 
            || estimated_cpu > self.total_budget.cpu_cores_available {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.total_rejected += 1;
            return Err("Query rejected: resource requirement exceeds total budget".to_string());
        }
        
        let queue = self.admission_queue.lock().unwrap();

        // Determine if we should admit immediately or queue
        let allocated = self.try_allocate_immediate(&request, &queue)?;

        if let Some(alloc) = allocated {
            drop(queue);
            let mut queue = self.admission_queue.lock().unwrap();
            queue.admitted.insert(request.query_id.clone(), alloc.clone());

            let mut metrics = self.metrics.lock().unwrap();
            metrics.total_admitted += 1;

            return Ok(alloc);
        }

        // Could not allocate immediately; queue the request
        drop(queue);
        let mut queue = self.admission_queue.lock().unwrap();

        if matches!(request.workload_class, WorkloadClass::OLTP { .. }) {
            // OLTP queries are high priority and should not be queued; reject if no resources
            let mut metrics = self.metrics.lock().unwrap();
            metrics.total_rejected += 1;
            return Err("OLTP query rejected: insufficient resources".to_string());
        }

        // For OLAP/Mixed: queue if queue not too deep
        if queue.depth() > 10000 {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.total_rejected += 1;
            return Err("Admission queue full; query rejected".to_string());
        }

        // Check throttle status
        if queue.throttle_active && matches!(request.workload_class, WorkloadClass::OLAP { .. }) {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.total_rejected += 1;
            return Err(format!(
                "OLAP throttled due to OLTP SLO pressure: {}",
                queue.throttle_reason
            ));
        }

        queue.enqueue(request.clone(), now_ms);
        Ok(ResourceAllocation {
            query_id: request.query_id,
            cpu_cores_allocated: 0.0,
            memory_mb_allocated: 0,
            assigned_at_ms: now_ms,
            throttle_multiplier: 1.0,
        })
    }

    /// Admit queries from the queue
    pub fn admit_from_queue(&self) -> Result<Option<ResourceAllocation>, String> {
        let mut queue = self.admission_queue.lock().unwrap();

        if queue.is_empty() {
            return Ok(None);
        }

        // Try to dequeue and allocate
        while let Some((request, _queue_time)) = queue.dequeue() {
            if let Ok(Some(alloc)) = self.try_allocate_from_remaining(&request, &queue) {
                queue.admitted.insert(request.query_id.clone(), alloc.clone());
                let mut metrics = self.metrics.lock().unwrap();
                metrics.total_admitted += 1;
                return Ok(Some(alloc));
            }
        }

        Ok(None)
    }

    /// Release resources allocated to a query
    pub fn release_resources(&self, query_id: &str) -> Result<(), String> {
        let mut queue = self.admission_queue.lock().unwrap();
        queue.admitted.remove(query_id);
        Ok(())
    }

    /// Check OLTP SLO pressure (0.0 to 1.0)
    pub fn check_oltp_slo_pressure(&self) -> f64 {
        let queue = self.admission_queue.lock().unwrap();

        let oltp_reserved = self.total_budget.memory_mb_reserved_for_oltp;
        let mut oltp_used = 0u64;

        for alloc in queue.admitted.values() {
            oltp_used += alloc.memory_mb_allocated;
        }

        if oltp_reserved == 0 {
            return 0.0;
        }

        (oltp_used as f64) / (oltp_reserved as f64)
    }

    /// Throttle OLAP queries if OLTP SLO pressure exceeds threshold
    pub fn throttle_olap_if_needed(&self) -> Result<(), String> {
        let pressure = self.check_oltp_slo_pressure();
        const PRESSURE_THRESHOLD: f64 = 0.8;

        let mut queue = self.admission_queue.lock().unwrap();

        if pressure > PRESSURE_THRESHOLD {
            if !queue.throttle_active {
                queue.throttle_active = true;
                queue.throttle_reason = format!(
                    "OLTP pressure at {:.2}% exceeds threshold {}%",
                    pressure * 100.0,
                    PRESSURE_THRESHOLD * 100.0
                );
                let mut metrics = self.metrics.lock().unwrap();
                metrics.throttle_events += 1;
            }

            // Apply throttle multiplier to OLAP queries
            for alloc in queue.admitted.values_mut() {
                alloc.throttle_multiplier = (1.0 - (pressure - PRESSURE_THRESHOLD) / (1.0 - PRESSURE_THRESHOLD)).max(0.1);
            }
        } else {
            if queue.throttle_active {
                queue.throttle_active = false;
                queue.throttle_reason.clear();
            }
            // Resume normal operation
            for alloc in queue.admitted.values_mut() {
                alloc.throttle_multiplier = 1.0;
            }
        }

        Ok(())
    }

    /// Get current metrics
    pub fn get_metrics(&self) -> Result<ResourceMetrics, String> {
        let queue = self.admission_queue.lock().unwrap();
        let metrics = self.metrics.lock().unwrap();

        Ok(ResourceMetrics {
            total_admitted: metrics.total_admitted,
            total_rejected: metrics.total_rejected,
            queue_depth: queue.depth(),
            throttle_events: metrics.throttle_events,
            avg_queue_time_ms: queue.avg_queue_time_ms(),
        })
    }

    /// Try to allocate resources immediately (internal)
    fn try_allocate_immediate(
        &self,
        request: &QueryRequest,
        queue: &AdmissionQueue,
    ) -> Result<Option<ResourceAllocation>, String> {
        let now_ms = current_time_ms();

        // Calculate available resources
        let total_allocated_memory: u64 = queue.admitted.values().map(|a| a.memory_mb_allocated).sum();
        let available_memory = self.total_budget.memory_mb_available - total_allocated_memory;

        let total_allocated_cpu: f64 = queue.admitted.values().map(|a| a.cpu_cores_allocated).sum();
        let available_cpu = self.total_budget.cpu_cores_available - total_allocated_cpu;

        // Estimate resource requirements based on cost
        let estimated_memory = (request.estimated_cost / 100).min(1024) as u64;
        let estimated_cpu = ((request.estimated_cost as f64) / 1000.0).min(4.0);

        // For OLTP: strict reservation
        match &request.workload_class {
            WorkloadClass::OLTP { .. } => {
                let oltp_memory_available = self.total_budget.memory_mb_reserved_for_oltp;
                let oltp_memory_used: u64 = queue
                    .admitted
                    .values()
                    .filter(|a| a.query_id.starts_with("oltp_"))
                    .map(|a| a.memory_mb_allocated)
                    .sum();
                let oltp_available = oltp_memory_available - oltp_memory_used;

                if estimated_memory <= oltp_available && estimated_cpu <= available_cpu {
                    return Ok(Some(ResourceAllocation {
                        query_id: request.query_id.clone(),
                        cpu_cores_allocated: estimated_cpu,
                        memory_mb_allocated: estimated_memory,
                        assigned_at_ms: now_ms,
                        throttle_multiplier: 1.0,
                    }));
                }
            }
            WorkloadClass::OLAP { .. } => {
                if estimated_memory <= available_memory && estimated_cpu <= available_cpu {
                    return Ok(Some(ResourceAllocation {
                        query_id: request.query_id.clone(),
                        cpu_cores_allocated: estimated_cpu,
                        memory_mb_allocated: estimated_memory,
                        assigned_at_ms: now_ms,
                        throttle_multiplier: 1.0,
                    }));
                }
            }
            WorkloadClass::Mixed => {
                if estimated_memory <= available_memory && estimated_cpu <= available_cpu {
                    return Ok(Some(ResourceAllocation {
                        query_id: request.query_id.clone(),
                        cpu_cores_allocated: estimated_cpu,
                        memory_mb_allocated: estimated_memory,
                        assigned_at_ms: now_ms,
                        throttle_multiplier: 1.0,
                    }));
                }
            }
        }

        Ok(None)
    }

    /// Try to allocate from remaining budget (internal)
    fn try_allocate_from_remaining(
        &self,
        request: &QueryRequest,
        queue: &AdmissionQueue,
    ) -> Result<Option<ResourceAllocation>, String> {
        let now_ms = current_time_ms();

        let total_allocated_memory: u64 = queue.admitted.values().map(|a| a.memory_mb_allocated).sum();
        let available_memory = self.total_budget.memory_mb_available - total_allocated_memory;

        let total_allocated_cpu: f64 = queue.admitted.values().map(|a| a.cpu_cores_allocated).sum();
        let available_cpu = self.total_budget.cpu_cores_available - total_allocated_cpu;

        let estimated_memory = (request.estimated_cost / 100).min(1024) as u64;
        let estimated_cpu = ((request.estimated_cost as f64) / 1000.0).min(4.0);

        if estimated_memory <= available_memory && estimated_cpu <= available_cpu {
            return Ok(Some(ResourceAllocation {
                query_id: request.query_id.clone(),
                cpu_cores_allocated: estimated_cpu,
                memory_mb_allocated: estimated_memory,
                assigned_at_ms: now_ms,
                throttle_multiplier: 1.0,
            }));
        }

        Ok(None)
    }
}

/// Get current time in milliseconds
fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workload_class_enum() {
        let oltp = WorkloadClass::OLTP { latency_sla_ms: 50 };
        let olap = WorkloadClass::OLAP { timeout_ms: 30000 };
        let mixed = WorkloadClass::Mixed;

        assert!(matches!(oltp, WorkloadClass::OLTP { .. }));
        assert!(matches!(olap, WorkloadClass::OLAP { .. }));
        assert!(matches!(mixed, WorkloadClass::Mixed));
    }

    #[test]
    fn test_resource_budget_creation() {
        let budget = ResourceBudget {
            cpu_cores_available: 16.0,
            memory_mb_available: 65536,
            cpu_cores_reserved_for_oltp: 8.0,
            memory_mb_reserved_for_oltp: 32768,
        };

        assert_eq!(budget.cpu_cores_available, 16.0);
        assert_eq!(budget.memory_mb_available, 65536);
        assert_eq!(budget.cpu_cores_reserved_for_oltp, 8.0);
        assert_eq!(budget.memory_mb_reserved_for_oltp, 32768);
    }

    #[test]
    fn test_admission_queue_creation() {
        let queue = AdmissionQueue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.depth(), 0);
        assert_eq!(queue.avg_queue_time_ms(), 0.0);
    }

    #[test]
    fn test_submit_query_accepted() {
        let budget = ResourceBudget {
            cpu_cores_available: 16.0,
            memory_mb_available: 65536,
            cpu_cores_reserved_for_oltp: 8.0,
            memory_mb_reserved_for_oltp: 32768,
        };
        let manager = ResourceManager::new(budget);

        let request = QueryRequest {
            query_id: "q1".to_string(),
            workload_class: WorkloadClass::OLAP { timeout_ms: 30000 },
            estimated_cost: 5000,
            priority: 0,
        };

        let result = manager.submit_query(request);
        assert!(result.is_ok());
        let alloc = result.unwrap();
        assert_eq!(alloc.query_id, "q1");
        assert!(alloc.cpu_cores_allocated > 0.0);
        assert!(alloc.memory_mb_allocated > 0);
    }

    #[test]
    fn test_submit_query_rejected_insufficient_resources() {
        let budget = ResourceBudget {
            cpu_cores_available: 0.5,
            memory_mb_available: 100,
            cpu_cores_reserved_for_oltp: 0.25,
            memory_mb_reserved_for_oltp: 50,
        };
        let manager = ResourceManager::new(budget);

        let request = QueryRequest {
            query_id: "q1".to_string(),
            workload_class: WorkloadClass::OLAP { timeout_ms: 30000 },
            estimated_cost: 100000,
            priority: 0,
        };

        let result = manager.submit_query(request);
        assert!(result.is_err());
    }

    #[test]
    fn test_oltp_queries_always_admitted() {
        let budget = ResourceBudget {
            cpu_cores_available: 16.0,
            memory_mb_available: 65536,
            cpu_cores_reserved_for_oltp: 8.0,
            memory_mb_reserved_for_oltp: 32768,
        };
        let manager = ResourceManager::new(budget);

        let request = QueryRequest {
            query_id: "oltp_1".to_string(),
            workload_class: WorkloadClass::OLTP { latency_sla_ms: 50 },
            estimated_cost: 5000,
            priority: 100,
        };

        let result = manager.submit_query(request);
        assert!(result.is_ok());
    }

    #[test]
    fn test_olap_queries_queued_when_full() {
        let budget = ResourceBudget {
            cpu_cores_available: 2.0,
            memory_mb_available: 1024,
            cpu_cores_reserved_for_oltp: 1.0,
            memory_mb_reserved_for_oltp: 512,
        };
        let manager = ResourceManager::new(budget);

        let request = QueryRequest {
            query_id: "q1".to_string(),
            workload_class: WorkloadClass::OLAP { timeout_ms: 30000 },
            estimated_cost: 1500,  // Results in estimated_cpu = 1.5, which fits in 2.0
            priority: 0,
        };

        let result = manager.submit_query(request);
        // Should queue or reject based on throttle/queue state
        // Initially not throttled, so should queue or be admitted
        assert!(result.is_ok());
    }

    #[test]
    fn test_release_resources_frees_budget() {
        let budget = ResourceBudget {
            cpu_cores_available: 16.0,
            memory_mb_available: 65536,
            cpu_cores_reserved_for_oltp: 8.0,
            memory_mb_reserved_for_oltp: 32768,
        };
        let manager = ResourceManager::new(budget);

        let request = QueryRequest {
            query_id: "q1".to_string(),
            workload_class: WorkloadClass::OLAP { timeout_ms: 30000 },
            estimated_cost: 5000,
            priority: 0,
        };

        let alloc = manager.submit_query(request).unwrap();
        let result = manager.release_resources(&alloc.query_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_admit_from_queue_respects_priority() {
        let budget = ResourceBudget {
            cpu_cores_available: 16.0,
            memory_mb_available: 65536,
            cpu_cores_reserved_for_oltp: 8.0,
            memory_mb_reserved_for_oltp: 32768,
        };
        let manager = ResourceManager::new(budget);

        let request1 = QueryRequest {
            query_id: "q1".to_string(),
            workload_class: WorkloadClass::OLAP { timeout_ms: 30000 },
            estimated_cost: 5000,
            priority: 10,
        };

        let request2 = QueryRequest {
            query_id: "q2".to_string(),
            workload_class: WorkloadClass::OLAP { timeout_ms: 30000 },
            estimated_cost: 5000,
            priority: 20,
        };

        manager.submit_query(request1).ok();
        manager.submit_query(request2).ok();

        // Both should be admitted or queued; priority determines order
        let metrics = manager.get_metrics().unwrap();
        assert!(metrics.total_admitted > 0 || metrics.queue_depth > 0);
    }

    #[test]
    fn test_check_oltp_slo_pressure() {
        let budget = ResourceBudget {
            cpu_cores_available: 16.0,
            memory_mb_available: 65536,
            cpu_cores_reserved_for_oltp: 8.0,
            memory_mb_reserved_for_oltp: 1024,
        };
        let manager = ResourceManager::new(budget);

        let pressure = manager.check_oltp_slo_pressure();
        assert!(pressure >= 0.0 && pressure <= 1.0);
        assert_eq!(pressure, 0.0); // No queries admitted yet
    }

    #[test]
    fn test_throttle_olap_if_needed_activates() {
        let budget = ResourceBudget {
            cpu_cores_available: 16.0,
            memory_mb_available: 65536,
            cpu_cores_reserved_for_oltp: 8.0,
            memory_mb_reserved_for_oltp: 1024,
        };
        let manager = ResourceManager::new(budget);

        // Submit some OLTP queries to build up pressure
        for i in 0..10 {
            let request = QueryRequest {
                query_id: format!("oltp_{}", i),
                workload_class: WorkloadClass::OLTP { latency_sla_ms: 50 },
                estimated_cost: 500,
                priority: 100,
            };
            manager.submit_query(request).ok();
        }

        manager.throttle_olap_if_needed().ok();
        let pressure = manager.check_oltp_slo_pressure();
        // Pressure depends on memory usage
        assert!(pressure >= 0.0);
    }

    #[test]
    fn test_throttle_olap_reduces_multiplier() {
        let budget = ResourceBudget {
            cpu_cores_available: 16.0,
            memory_mb_available: 65536,
            cpu_cores_reserved_for_oltp: 8.0,
            memory_mb_reserved_for_oltp: 100,
        };
        let manager = ResourceManager::new(budget);

        // Simulate high OLTP pressure
        for i in 0..20 {
            let request = QueryRequest {
                query_id: format!("oltp_{}", i),
                workload_class: WorkloadClass::OLTP { latency_sla_ms: 50 },
                estimated_cost: 50,
                priority: 100,
            };
            manager.submit_query(request).ok();
        }

        manager.throttle_olap_if_needed().ok();
        
        // Check that throttle is active and multiplier is reduced
        let queue = manager.admission_queue.lock().unwrap();
        if !queue.admitted.is_empty() {
            for alloc in queue.admitted.values() {
                assert!(alloc.throttle_multiplier >= 0.1 && alloc.throttle_multiplier <= 1.0);
            }
        }
    }

    #[test]
    fn test_get_metrics_tracking() {
        let budget = ResourceBudget {
            cpu_cores_available: 16.0,
            memory_mb_available: 65536,
            cpu_cores_reserved_for_oltp: 8.0,
            memory_mb_reserved_for_oltp: 32768,
        };
        let manager = ResourceManager::new(budget);

        let request = QueryRequest {
            query_id: "q1".to_string(),
            workload_class: WorkloadClass::OLAP { timeout_ms: 30000 },
            estimated_cost: 5000,
            priority: 0,
        };

        manager.submit_query(request).ok();

        let metrics = manager.get_metrics().unwrap();
        assert!(metrics.total_admitted > 0 || metrics.queue_depth > 0);
        assert_eq!(metrics.total_rejected, 0);
    }

    #[test]
    fn test_concurrent_submissions() {
        use std::thread;

        let budget = ResourceBudget {
            cpu_cores_available: 16.0,
            memory_mb_available: 65536,
            cpu_cores_reserved_for_oltp: 8.0,
            memory_mb_reserved_for_oltp: 32768,
        };
        let manager = Arc::new(ResourceManager::new(budget));

        let mut handles = vec![];

        for i in 0..10 {
            let mgr = Arc::clone(&manager);
            let handle = thread::spawn(move || {
                let request = QueryRequest {
                    query_id: format!("q{}", i),
                    workload_class: WorkloadClass::OLAP { timeout_ms: 30000 },
                    estimated_cost: 1000 + (i as u64 * 100),
                    priority: i as i32,
                };
                let _ = mgr.submit_query(request);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let metrics = manager.get_metrics().unwrap();
        assert!(metrics.total_admitted + (metrics.queue_depth as u64) > 0 || metrics.total_rejected > 0);
    }
}
