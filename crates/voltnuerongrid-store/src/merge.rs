use crate::types::{SegmentId, CommitTs, SnapshotTs};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Merge policy configuration for a segment
#[derive(Debug, Clone)]
pub struct MergePolicy {
    /// Maximum number of tail versions before triggering merge
    pub max_tail_versions: usize,
    /// Maximum estimated bytes in tail before triggering merge
    pub max_tail_bytes: u64,
    /// Maximum staleness (ms) tolerated for SLA before urgent merge
    pub max_staleness_ms: u64,
    /// If tail is idle for this duration (ms), attempt lazy merge
    pub idle_merge_ms: u64,
}

impl Default for MergePolicy {
    fn default() -> Self {
        MergePolicy {
            max_tail_versions: 100,
            max_tail_bytes: 10 * 1024 * 1024, // 10MB
            max_staleness_ms: 30_000,           // 30s
            idle_merge_ms: 5_000,               // 5s
        }
    }
}

/// A scheduled merge job for a segment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MergeJobId(pub u64);

/// Merge job specification
#[derive(Debug, Clone)]
pub struct MergeJob {
    pub job_id: MergeJobId,
    pub segment_id: SegmentId,
    /// Snapshot timestamp to compute visible versions at
    pub snapshot_ts: SnapshotTs,
    /// Include all tail versions up to this commit timestamp
    pub window_end_ts: CommitTs,
    /// Created timestamp (ms since epoch)
    pub created_at_ms: u64,
    /// Started timestamp (if running)
    pub started_at_ms: Option<u64>,
    /// Completed timestamp (if done)
    pub completed_at_ms: Option<u64>,
}

/// Merge job status/result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl MergeJob {
    pub fn new(
        job_id: MergeJobId,
        segment_id: SegmentId,
        snapshot_ts: SnapshotTs,
        window_end_ts: CommitTs,
    ) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        MergeJob {
            job_id,
            segment_id,
            snapshot_ts,
            window_end_ts,
            created_at_ms: now_ms,
            started_at_ms: None,
            completed_at_ms: None,
        }
    }

    pub fn duration_ms(&self) -> Option<u64> {
        match (self.started_at_ms, self.completed_at_ms) {
            (Some(start), Some(end)) => Some(end.saturating_sub(start)),
            _ => None,
        }
    }

    pub fn age_ms(&self) -> u64 {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        now_ms.saturating_sub(self.created_at_ms)
    }
}

/// Merge operation metrics
#[derive(Debug, Clone, Default)]
pub struct MergeMetrics {
    /// Total merge jobs completed
    pub jobs_completed: u64,
    /// Total merge jobs failed
    pub jobs_failed: u64,
    /// Average merge duration (ms)
    pub avg_merge_duration_ms: u64,
    /// Latest merge lag (ms between SLA breach and merge start)
    pub latest_merge_lag_ms: u64,
    /// Estimated tail bytes awaiting merge
    pub pending_tail_bytes: u64,
    /// Estimated tail versions awaiting merge
    pub pending_tail_versions: u64,
}

/// Merge phase for progress tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergePhase {
    /// Acquiring segment and snapshot locks
    AcquiringLocks,
    /// Scanning tail versions in window
    ScanningTail,
    /// Computing latest visible version per row
    ComputingVisibility,
    /// Writing to base storage
    WritingBase,
    /// Swapping old base with new (atomic point)
    SwappingManifest,
    /// Marking old tail versions as reclaimable
    MarkingObsolete,
    /// Complete
    Done,
}

/// Progress update during merge
#[derive(Debug, Clone)]
pub struct MergeProgress {
    pub job_id: MergeJobId,
    pub phase: MergePhase,
    pub rows_processed: u64,
    pub rows_merged: u64,
    pub elapsed_ms: u64,
}

/// Background merge manager for tail-to-base consolidation
pub struct MergeManager {
    /// Policy for all segments
    _policy: MergePolicy,
    /// Pending merge jobs (job_id -> job)
    pending_jobs: Arc<Mutex<VecDeque<MergeJob>>>,
    /// Active merge jobs (segment_id -> job)
    active_jobs: Arc<Mutex<HashMap<SegmentId, MergeJob>>>,
    /// Completed merge jobs (ring buffer of recent)
    completed_jobs: Arc<Mutex<VecDeque<(MergeJob, MergeMetrics)>>>,
    /// Merge metrics
    metrics: Arc<Mutex<MergeMetrics>>,
    /// Next job ID counter
    next_job_id: Arc<AtomicU64>,
    /// Max completed jobs to retain
    max_history: usize,
}

impl MergeManager {
    pub fn new(policy: MergePolicy, max_history: usize) -> Self {
        MergeManager {
            _policy: policy,
            pending_jobs: Arc::new(Mutex::new(VecDeque::new())),
            active_jobs: Arc::new(Mutex::new(HashMap::new())),
            completed_jobs: Arc::new(Mutex::new(VecDeque::new())),
            metrics: Arc::new(Mutex::new(MergeMetrics::default())),
            next_job_id: Arc::new(AtomicU64::new(1)),
            max_history,
        }
    }

    /// Schedule a merge job for a segment
    pub fn schedule_merge(
        &self,
        segment_id: SegmentId,
        snapshot_ts: SnapshotTs,
        window_end_ts: CommitTs,
    ) -> Result<MergeJobId, String> {
        let job_id = MergeJobId(self.next_job_id.fetch_add(1, Ordering::SeqCst));
        let job = MergeJob::new(job_id, segment_id, snapshot_ts, window_end_ts);

        self.pending_jobs
            .lock()
            .map_err(|e| e.to_string())?
            .push_back(job);

        Ok(job_id)
    }

    /// Get next pending merge job (non-blocking)
    pub fn take_next_job(&self) -> Result<Option<MergeJob>, String> {
        Ok(self
            .pending_jobs
            .lock()
            .map_err(|e| e.to_string())?
            .pop_front())
    }

    /// Mark job as started (called after take_next_job, requires the MergeJob)
    pub fn start_job(&self, job: MergeJob) -> Result<(), String> {
        let mut job = job;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        job.started_at_ms = Some(now_ms);

        self.active_jobs
            .lock()
            .map_err(|e| e.to_string())?
            .insert(job.segment_id, job);

        Ok(())
    }

    /// Mark job as completed
    pub fn complete_job(&self, job_id: MergeJobId, metrics: MergeMetrics) -> Result<(), String> {
        let mut job = None;
        let mut active = self.active_jobs.lock().map_err(|e| e.to_string())?;

        for (_, j) in active.iter_mut() {
            if j.job_id == job_id {
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                j.completed_at_ms = Some(now_ms);
                job = Some(j.clone());
                break;
            }
        }

        if let Some(j) = job {
            active.remove(&j.segment_id);
            drop(active);

            let mut completed = self.completed_jobs.lock().map_err(|e| e.to_string())?;
            completed.push_back((j, metrics));

            if completed.len() > self.max_history {
                completed.pop_front();
            }

            let mut m = self.metrics.lock().map_err(|e| e.to_string())?;
            m.jobs_completed += 1;
        }

        Ok(())
    }

    /// Mark job as failed
    pub fn fail_job(&self, job_id: MergeJobId) -> Result<(), String> {
        let mut active = self.active_jobs.lock().map_err(|e| e.to_string())?;

        for (_, job) in active.iter_mut() {
            if job.job_id == job_id {
                let mut m = self.metrics.lock().map_err(|e| e.to_string())?;
                m.jobs_failed += 1;
                break;
            }
        }

        Ok(())
    }

    /// Get merge job status
    pub fn get_job_status(&self, job_id: MergeJobId) -> Result<Option<MergeStatus>, String> {
        if let Ok(pending) = self.pending_jobs.lock() {
            if pending.iter().any(|j| j.job_id == job_id) {
                return Ok(Some(MergeStatus::Pending));
            }
        }

        if let Ok(active) = self.active_jobs.lock() {
            if active.values().any(|j| j.job_id == job_id) {
                return Ok(Some(MergeStatus::Running));
            }
        }

        if let Ok(completed) = self.completed_jobs.lock() {
            if completed.iter().any(|(j, _)| j.job_id == job_id) {
                return Ok(Some(MergeStatus::Completed));
            }
        }

        Ok(None)
    }

    /// Get current metrics
    pub fn get_metrics(&self) -> Result<MergeMetrics, String> {
        self.metrics
            .lock()
            .map_err(|e| e.to_string())
            .map(|m| m.clone())
    }

    /// List pending jobs
    pub fn list_pending(&self) -> Result<Vec<MergeJob>, String> {
        Ok(self
            .pending_jobs
            .lock()
            .map_err(|e| e.to_string())?
            .iter()
            .cloned()
            .collect())
    }

    /// List active jobs
    pub fn list_active(&self) -> Result<Vec<MergeJob>, String> {
        Ok(self
            .active_jobs
            .lock()
            .map_err(|e| e.to_string())?
            .values()
            .cloned()
            .collect())
    }
}

impl Default for MergeManager {
    fn default() -> Self {
        MergeManager::new(MergePolicy::default(), 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_policy_defaults() {
        let policy = MergePolicy::default();
        assert_eq!(policy.max_tail_versions, 100);
        assert_eq!(policy.max_tail_bytes, 10 * 1024 * 1024);
        assert_eq!(policy.max_staleness_ms, 30_000);
        assert_eq!(policy.idle_merge_ms, 5_000);
    }

    #[test]
    fn test_merge_job_creation() {
        let job = MergeJob::new(
            MergeJobId(1),
            SegmentId(1),
            SnapshotTs(100),
            CommitTs(200),
        );
        assert_eq!(job.job_id, MergeJobId(1));
        assert_eq!(job.segment_id, SegmentId(1));
        assert!(job.started_at_ms.is_none());
        assert!(job.completed_at_ms.is_none());
    }

    #[test]
    fn test_merge_manager_schedule() {
        let mgr = MergeManager::default();
        let job_id = mgr
            .schedule_merge(SegmentId(1), SnapshotTs(100), CommitTs(200))
            .unwrap();
        assert_eq!(job_id.0, 1);

        let pending = mgr.list_pending().unwrap();
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn test_merge_manager_take_next() {
        let mgr = MergeManager::default();
        mgr.schedule_merge(SegmentId(1), SnapshotTs(100), CommitTs(200))
            .unwrap();

        let job = mgr.take_next_job().unwrap();
        assert!(job.is_some());

        let pending = mgr.list_pending().unwrap();
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn test_merge_manager_complete_job() {
        let mgr = MergeManager::default();
        let job_id = mgr
            .schedule_merge(SegmentId(1), SnapshotTs(100), CommitTs(200))
            .unwrap();

        let job = mgr.take_next_job().unwrap().unwrap();
        mgr.start_job(job).unwrap();

        let metrics = MergeMetrics::default();
        mgr.complete_job(job_id, metrics).unwrap();

        let status = mgr.get_job_status(job_id).unwrap();
        assert_eq!(status, Some(MergeStatus::Completed));
    }

    #[test]
    fn test_merge_manager_metrics() {
        let mgr = MergeManager::default();
        mgr.schedule_merge(SegmentId(1), SnapshotTs(100), CommitTs(200))
            .unwrap();

        let metrics = mgr.get_metrics().unwrap();
        assert_eq!(metrics.jobs_completed, 0);
        assert_eq!(metrics.jobs_failed, 0);
    }

    #[test]
    fn test_merge_job_age() {
        let job = MergeJob::new(
            MergeJobId(1),
            SegmentId(1),
            SnapshotTs(100),
            CommitTs(200),
        );
        let age = job.age_ms();
        assert!(age >= 0);
    }

    #[test]
    fn test_merge_status_enum() {
        assert_ne!(MergeStatus::Pending, MergeStatus::Running);
        assert_ne!(MergeStatus::Running, MergeStatus::Completed);
    }

    #[test]
    fn test_merge_manager_fail_job() {
        let mgr = MergeManager::default();
        let job_id = mgr
            .schedule_merge(SegmentId(1), SnapshotTs(100), CommitTs(200))
            .unwrap();

        let job = mgr.take_next_job().unwrap().unwrap();
        mgr.start_job(job).unwrap();

        mgr.fail_job(job_id).unwrap();
        
        let metrics = mgr.get_metrics().unwrap();
        assert_eq!(metrics.jobs_failed, 1);
    }

    #[test]
    fn test_merge_manager_multiple_jobs() {
        let mgr = MergeManager::default();
        
        let job_id_1 = mgr
            .schedule_merge(SegmentId(1), SnapshotTs(100), CommitTs(200))
            .unwrap();
        let job_id_2 = mgr
            .schedule_merge(SegmentId(2), SnapshotTs(100), CommitTs(300))
            .unwrap();

        let pending = mgr.list_pending().unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(job_id_1.0, 1);
        assert_eq!(job_id_2.0, 2);
    }

    #[test]
    fn test_merge_job_duration() {
        let job = MergeJob::new(
            MergeJobId(1),
            SegmentId(1),
            SnapshotTs(100),
            CommitTs(200),
        );
        
        assert!(job.duration_ms().is_none());
    }

    #[test]
    fn test_merge_metrics_default() {
        let metrics = MergeMetrics::default();
        assert_eq!(metrics.jobs_completed, 0);
        assert_eq!(metrics.jobs_failed, 0);
        assert_eq!(metrics.avg_merge_duration_ms, 0);
        assert_eq!(metrics.latest_merge_lag_ms, 0);
    }

    #[test]
    fn test_merge_phase_enum() {
        assert_eq!(MergePhase::AcquiringLocks, MergePhase::AcquiringLocks);
        assert_ne!(MergePhase::AcquiringLocks, MergePhase::Done);
    }

    #[test]
    fn test_merge_job_id_copy() {
        let id1 = MergeJobId(42);
        let id2 = id1;
        assert_eq!(id1, id2);
    }
}
