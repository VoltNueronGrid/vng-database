Please go through the repo. I suspect this code is NOT production based code. There are lot of gaps. First and foremost any database should respect codd's 12 pricinciples. Support SQL (Structured Query Language) to manage data efficiently, ensuring accuracy (ACID compliance) and reducing redundancy.

- All the data should be persistent for each database and we cannot loose any data for DB restarts or crashes.
- All the data should be written to the files and read from the files with high performance and using parallel reads/writes - which can also be fine-tuned. You can suggest better idea if not in files to avoid crashes to maintain peristence.
- Database and connections are different.
- We can have any number of connections to a database.
- We can create any number of databases in each connection. 
- Each database can have its own users and roles.
- Each database should not be repeated i.e., if already database is created, we cannot create another database with the same name.
- Similarly, inside a database, we cannot have any duplicate names for schemas or tables or columns or views or triggers or events or functions etc. It is per database, so database is the boundary.
- If I login into a database using a connection and have previleges, then I should be able to manipulate all the database objects.
- I can have any number of connections to a database (limited by memory and cpu) and these connections can be part of a connection pool or direct connection. We should have options to modify or finetune the connection parameters
- I should be able to retrieve the data from the database using http/https/native drivers. The drivers should be able to connect and retrieve the data very fast. It can use internal paging if needed or limit number of rows as per the queries.
- I need ability to fine tune the database - using a service or configuration file or we can have a system schema (called as metadata schema - a separate schema) in each database where all the parameters and information is stored, which is created for every database by default. Similar to information_schema and pg_catalog schema in postgres.
- Since this is HTAP database, we need to have similar functionalities for OLAP based database as well.
- Please add all those functionalities needed for OLAP as well.

UI:
- Also, in the StudioAdmin UI, we need capabilities support all the above, which should be tested thoroughly. If needed use Playwright agent and complete all the above functionalities without any defects.
- Also please check the studio-design.html (ui/voltnuerongrid-studio/design/studio-design.html) and check the UI styles. There are a lot of gaps in the currently implementation including the colors. Please fix this.

Please modify the Backend and Frontend if needed. Use cursor and github copilot instructions and rules.

Please write a remaining.md if you are running out of tokens or hitting session limits.


========


Storage substrate: are you OK with adopting RocksDB for durable persistence, or do you specifically want a from-scratch Rust storage engine? RocksDB is the realistic path; from-scratch is a year of work.
- [pavan] - Yes, but I need a configuration, where I can select RocksDB or VNG. Idea is we can start with RocksDB now and later implement our own like Cockroach. Also, RocksDB is written in C++ which means, if we have to fix any issues w.r.t RocksDB, I am not sure how much complex it would be and compatible wit Rust. So, let it be configuration wise with default as RocksDB for now. If user chooses to VNG, we can just mention that it is NOT support for now and will be available in future releases.

SQL engine: are you OK with adopting DataFusion + sqlparser-rs for execution & parsing, or do you want to keep the bespoke pieces? DataFusion would replace voltnuerongrid-exec, voltnuerongrid-opt, and most of the SQL execution surface — big refactor, but turns "doesn't work" into "works correctly" overnight.
- [pavan] - Yes, but I need a configuration, where I can select DataFusion + sqlparser-rs or VNG. Idea is we can start with DataFusion + sqlparser-rs now and later implement our own logic. So, let it be configuration wise with default as DataFusion + sqlparser-rs for now. If user chooses to VNG, we can just mention that it is NOT support for now and will be available in future releases.


Scope priority: if I have to pick ONE of {durable storage, real SQL execution, multi-DB + users, OLAP path, drivers, UI parity} to land first, what do you want to demo end-to-end?
- [pavan] - If I have to make this a production grade, I need all of them. However, we can go by this priority:
durable storage
real SQL execution
UI parity
drivers
multi-DB + users
OLAP path

Backwards compatibility: are the existing 311 HTTP routes a contract you need to keep, or are some of them legacy / experimental that I can prune in the refactor?
- [pavan]: Yes, please go ahead

The 33k-line main.rs refactor: can I split it into modules in a separate PR before functional work? It's a prerequisite for clean changes after.
- [pavan] - Yes, please go ahead and refactor for modular, clean-code, maintainable code, following reusability, OOPs, backward compability and SOLID principles.



====
Next:
1. Drivers should work natively.
2. Performance is the peak. if possible, generate an idea for fast insertions, fast updates and fast retrivals. combine vector and Sql and nosql concepts if necessary.
3. Review the code to make sure no gaps and this should be really a performant grade database.
4. Make sure we support New SQL as well (future requirement)
5. Make sure we support multi-model
6. We need database agents
7. We need native UI, not just thin client
8. We need clone schema support with trillion rows per table
9. Introduce versioning of database resources if possible - new concept




====

What's left (recommended next session) - please refer to #remaining.md for details.

Phase 2 — RocksDB is now the highest leverage item. Reasons:

Closes the durability gap (the in-memory "WAL" that uses flush() not fsync).
Forces the MSRV bump, which unblocks real DataFusion adoption.
Phase 0's config selector for RocksDB | VNG is already wired and waiting.

The full handoff is in remaining.md on the branch and commit and push to origin.

Caveat (same as last 4 sessions)
The service main.rs integration was not compile-tested locally — only the new crate and the other workspace crates were. Please complete and fix if any further issues come up.