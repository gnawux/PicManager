-- Fix orphaned people nodes whose parent_id points to themselves (caused by
-- merging a parent into one of its children before the promotion logic was
-- in place).  Such nodes are invisible in the UI because they are neither
-- top-level (parent_id IS NULL) nor reachable from a top-level ancestor.
UPDATE people SET parent_id = NULL WHERE id = parent_id;
