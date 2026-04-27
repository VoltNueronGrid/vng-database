My original requirement is:
I would like to get a design of Studio UI. Can you design a very nice, intuitive, simple, modern and responsive UI for this ? Also, i would need a desktop application. So, please create the design first and based on that, we can develop

Back ground information:

Using Claude design, I have done below:


What works:

    Welcome screen → Quick Connect, dashboard launcher, recent connections
    Title bar with traffic lights, active-connection badge, theme menu (light / dark / system), refresh, disconnect, dashboard, settings
    Sidebar tabs: Schema (connections + tree), Users & Roles, History, Saved
    Schema tree — expandable databases / schemas / tables, double-click to open table tab, columns with type chips and PK markers
    Workspace — multi-tab editor, run button with route badge (OLTP / OLAP / Hybrid), results table with 25 sample rows, plan/messages/history tabs
    Right panel — table stats, columns list, quick actions (SELECT, DDL, Truncate)
    Right-click context menus on connections, databases, schemas, tables, columns, users — including the nested INSERT/UPDATE/DELETE submenu
    Resource modals — Create Database / Schema / Table (full column editor), Drop confirmations with type-to-confirm, Truncate, Create User, Grant Role, View DDL
    Connection panel — server type cards, auth modes, test button with simulated success/failure
    Dashboard — KPIs, node health bars, audit log
    Status bar — route, latency, row count, theme cycle button

Caveats:
    The src/ TypeScript tree is the production implementation reference; several files (Workspace, ConnectionPanel, Dashboard, Welcome, ErrorBoundary, several stores/hooks, api/studio-client) still need to be authored to match the preview before it'll build as a real Tauri app.
    All data is mocked from preview/data.js.

Next steps:
Flesh out the missing TypeScript components, wire studio-client to real backend endpoints, and replace the textarea editor with Monaco and replace data with actual API endpoints (create/update APIs if needed on both backend (RUST) and frontend)

Please make sure to follow SOLID principles and OOPS so that it is extensible as much as possible. Create reusable functions as much as possible.