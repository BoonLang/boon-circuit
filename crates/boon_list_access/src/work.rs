use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkLimits {
    pub max_index_seeks: u64,
    pub max_key_ranges: u64,
    pub max_keys_visited: u64,
    pub max_candidates_visited: u64,
    pub max_rows_returned: u64,
    pub max_branch_polls: u64,
    pub max_full_scans: u64,
}

impl WorkLimits {
    pub const fn new(
        max_index_seeks: u64,
        max_key_ranges: u64,
        max_keys_visited: u64,
        max_candidates_visited: u64,
        max_rows_returned: u64,
        max_branch_polls: u64,
        max_full_scans: u64,
    ) -> Self {
        Self {
            max_index_seeks,
            max_key_ranges,
            max_keys_visited,
            max_candidates_visited,
            max_rows_returned,
            max_branch_polls,
            max_full_scans,
        }
    }
}

impl Default for WorkLimits {
    fn default() -> Self {
        Self::new(256, 256, 100_000, 100_000, 10_000, 400_000, 1)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AccessMetrics {
    pub index_seeks: u64,
    pub key_ranges: u64,
    pub keys_visited: u64,
    pub candidates_visited: u64,
    pub rows_returned: u64,
    pub cursor_seeks: u64,
    pub branch_polls: u64,
    pub union_duplicates_skipped: u64,
    pub intersection_candidates_skipped: u64,
    pub full_scans: u64,
    pub work_limit_failures: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitKind {
    IndexSeeks,
    KeyRanges,
    KeysVisited,
    CandidatesVisited,
    RowsReturned,
    BranchPolls,
    FullScans,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkLimitExceeded {
    pub kind: LimitKind,
    pub limit: u64,
}

impl fmt::Display for WorkLimitExceeded {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "access work limit {:?} exceeded at {}",
            self.kind, self.limit
        )
    }
}

impl Error for WorkLimitExceeded {}

#[derive(Clone, Debug)]
pub struct WorkTracker {
    limits: WorkLimits,
    metrics: AccessMetrics,
}

impl WorkTracker {
    pub const fn new(limits: WorkLimits) -> Self {
        Self {
            limits,
            metrics: AccessMetrics {
                index_seeks: 0,
                key_ranges: 0,
                keys_visited: 0,
                candidates_visited: 0,
                rows_returned: 0,
                cursor_seeks: 0,
                branch_polls: 0,
                union_duplicates_skipped: 0,
                intersection_candidates_skipped: 0,
                full_scans: 0,
                work_limit_failures: 0,
            },
        }
    }

    pub const fn limits(&self) -> WorkLimits {
        self.limits
    }

    pub const fn metrics(&self) -> AccessMetrics {
        self.metrics
    }

    pub fn reset(&mut self) {
        self.metrics = AccessMetrics::default();
    }

    pub(crate) fn begin_seek(
        &mut self,
        cursor: bool,
        full_scan: bool,
    ) -> Result<(), WorkLimitExceeded> {
        Self::bump(
            &mut self.metrics.index_seeks,
            self.limits.max_index_seeks,
            LimitKind::IndexSeeks,
            &mut self.metrics.work_limit_failures,
        )?;
        Self::bump(
            &mut self.metrics.key_ranges,
            self.limits.max_key_ranges,
            LimitKind::KeyRanges,
            &mut self.metrics.work_limit_failures,
        )?;
        if cursor {
            self.metrics.cursor_seeks = self.metrics.cursor_seeks.saturating_add(1);
        }
        if full_scan {
            Self::bump(
                &mut self.metrics.full_scans,
                self.limits.max_full_scans,
                LimitKind::FullScans,
                &mut self.metrics.work_limit_failures,
            )?;
        }
        Ok(())
    }

    pub(crate) fn visit_key(&mut self) -> Result<(), WorkLimitExceeded> {
        Self::bump(
            &mut self.metrics.keys_visited,
            self.limits.max_keys_visited,
            LimitKind::KeysVisited,
            &mut self.metrics.work_limit_failures,
        )
    }

    pub(crate) fn visit_candidate(&mut self) -> Result<(), WorkLimitExceeded> {
        Self::bump(
            &mut self.metrics.candidates_visited,
            self.limits.max_candidates_visited,
            LimitKind::CandidatesVisited,
            &mut self.metrics.work_limit_failures,
        )
    }

    pub(crate) fn return_row(&mut self) -> Result<(), WorkLimitExceeded> {
        Self::bump(
            &mut self.metrics.rows_returned,
            self.limits.max_rows_returned,
            LimitKind::RowsReturned,
            &mut self.metrics.work_limit_failures,
        )
    }

    pub(crate) fn poll_branch(&mut self) -> Result<(), WorkLimitExceeded> {
        Self::bump(
            &mut self.metrics.branch_polls,
            self.limits.max_branch_polls,
            LimitKind::BranchPolls,
            &mut self.metrics.work_limit_failures,
        )
    }

    pub(crate) fn skip_union_duplicates(&mut self, count: u64) {
        self.metrics.union_duplicates_skipped =
            self.metrics.union_duplicates_skipped.saturating_add(count);
    }

    pub(crate) fn skip_intersection_candidate(&mut self) {
        self.metrics.intersection_candidates_skipped = self
            .metrics
            .intersection_candidates_skipped
            .saturating_add(1);
    }

    fn bump(
        counter: &mut u64,
        limit: u64,
        kind: LimitKind,
        failures: &mut u64,
    ) -> Result<(), WorkLimitExceeded> {
        if *counter >= limit {
            *failures = failures.saturating_add(1);
            return Err(WorkLimitExceeded { kind, limit });
        }
        *counter += 1;
        Ok(())
    }
}
