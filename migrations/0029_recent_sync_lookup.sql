-- Recent synchronization is ordered by durable item completion, not source metadata
-- updates. Full inventory scans refresh source rows and must not make old photos look
-- newly synchronized.
CREATE INDEX idx_sync_items_recent_success
    ON sync_items(finished_at DESC, source_id)
    WHERE status = 'succeeded' AND operation = 'export_original';
