-- Phase 2.5 continuation: how far a portal's list has been read.
--
-- Only providers with an explicit page advance it (Eightfold offsets, a
-- selector URL template). A portal whose config has neither leaves it at 0.
ALTER TABLE portals ADD COLUMN last_page_fetched INTEGER NOT NULL DEFAULT 0;
