-- Feature examples: USE, WITH RECURSIVE, MERGE, multi-line INSERT
-- Run these against a running voltnuerongridd instance (HTTP API)

-- Using the default database; avoid CREATE DATABASE/USE in this example

-- Create supporting tables
CREATE TABLE public.multi_insert (id INT, txt TEXT);
CREATE TABLE public.merge_target (id INT PRIMARY KEY, val TEXT);

-- Multi-line INSERT: multiple rows across lines
INSERT INTO public.multi_insert (id, txt) VALUES
  (1, 'one'),
  (2, 'two'),
  (3, 'three');

-- Recursive queries (WITH RECURSIVE) are not implemented; use generate_series as an alternative
SELECT generate_series(1,5) AS n;

-- MERGE is not supported; use INSERT ... ON CONFLICT as an upsert alternative
INSERT INTO public.merge_target (id,val) VALUES (1,'a'), (2,'b')
  ON CONFLICT (id) DO UPDATE SET val = EXCLUDED.val;

-- Show results
SELECT * FROM public.multi_insert;
SELECT * FROM public.merge_target;

-- Cleanup (optional) -- remove created tables if desired
DROP TABLE IF EXISTS public.multi_insert;
DROP TABLE IF EXISTS public.merge_target;
