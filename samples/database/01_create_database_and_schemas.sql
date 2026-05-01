-- VoltNueronGrid Sample Database Setup
-- Comprehensive HTAP (Hybrid Transactional/Analytical Processing) Demo Database
-- This sample demonstrates both OLTP and OLAP capabilities

-- =============================================
-- 1. CREATE DATABASE AND SCHEMAS
-- =============================================

-- Create the main demo database
CREATE DATABASE voltnuerongrid_demo;

-- Switch to the demo database
USE voltnuerongrid_demo;

-- Create schemas for different purposes
CREATE SCHEMA oltp;          -- For transactional workloads
CREATE SCHEMA olap;          -- For analytical workloads  
CREATE SCHEMA reporting;     -- For reporting and dashboards
CREATE SCHEMA staging;       -- For ETL and data staging
CREATE SCHEMA audit;         -- For audit trails and logging
CREATE SCHEMA ai;            -- For AI/ML features and vector data
CREATE SCHEMA plugins;       -- For plugin-related objects

-- Create sample users and roles
CREATE USER demo_admin WITH PASSWORD 'admin123' SUPERUSER;
CREATE USER demo_analyst WITH PASSWORD 'analyst123';
CREATE USER demo_developer WITH PASSWORD 'dev123';
CREATE USER demo_etl WITH PASSWORD 'etl123';

-- Create roles
CREATE ROLE oltp_reader;
CREATE ROLE olap_writer; 
CREATE ROLE reporting_viewer;
CREATE ROLE plugin_manager;

-- Grant schema permissions
GRANT ALL ON SCHEMA oltp TO demo_admin;
GRANT USAGE ON SCHEMA oltp TO oltp_reader;
GRANT ALL ON SCHEMA olap TO demo_admin;
GRANT USAGE, CREATE ON SCHEMA olap TO olap_writer;
GRANT USAGE ON SCHEMA reporting TO reporting_viewer;
GRANT ALL ON SCHEMA ai TO demo_admin;
GRANT ALL ON SCHEMA plugins TO plugin_manager;

-- Grant roles to users
GRANT oltp_reader TO demo_analyst;
GRANT olap_writer TO demo_developer;
GRANT reporting_viewer TO demo_analyst, demo_developer;
GRANT plugin_manager TO demo_admin;