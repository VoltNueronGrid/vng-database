Imagine that you are a Enterprise Architect with System Programming knowledge having 25 years at Microsoft.

Now that you have complete picture of MDAP workspace, can we design and architect a database that can hold OLAP data with multiple dimensions in RUST language.

Primary Intent - is to ingest the data into the database engine and connect the MDAP to the database engine instead of in-memory. The retrieval of data should be extemely blazingly fast. The database engine should support any number of concurrent users (with auto-scale) and should have THE BEST memory management using less memory.

Below are some of the features to start with:
1. It should be ANSI-SQL complaint, have native AI inbuilt to chat and extract, ingest, import, export data.
2. Users should be able to create a database, create tables, views, materialized views, functions
3. We should have the ability to write inbuilt functions using Rush or Javascript ES6 or Python inside the database
4. We need to have a datbase engine which can have multiple instances, can have high availability, fault tolerance, reliable, 0 crashes, elastic (on cloud), should support multiple languages (internationalization), should have UTF-8 support.
5. Data file and database engine can be separate like Oracle database
6. We should be able to ingest or import the data from CSV, Parquet or Excel files
7. This import should be extremely fast with multi threading support
8. Should be able to run locally on laptop or on cloud with native SaaS
9. Should be extensible like Postgres i.e, we have plugins to extend the database for vector support (for AI), plugins for geospatial, plugins for search, plugins multimodel like support document, graph, wide column, plugins to cache like Redis etc.
10. Should support trillions of rows per table, with ease and support large volumes of data but should be able to retrieve the data extremely fast in nanosecond even with such huge volumes of data. Think of somethink like sharding etc.
11. Should support indexes and constraints
12. Should support triggers for each table and action on table like before-insert, after-insert, before-update, after-update, before-delete, after-delete, before-truncate, after-truncate, before-drop-table, after-drop-table, before-create-table, after-create-table, before-create-view, after-create-view.
    a. Each of these triggers can have call a database function that can check for something like pre-requisites or conditions or perform post actions.
    b. These triggers should also be able to send an event along with event data to a Kafka or NATS or any other queue, which can be used for any other actions or for monitoring.
13. Retrieval of rows should be extremely fast for each table even if they have trillion in each of them, even if they have joins. So, create any new algorithm or retrieval mechanism if needed or paging (to and from memory to disk)
13. Please check plan-plat-pivotmdap, where we have functions - all those functions should  be available natively in the database as seeded functions and also have support to user defined functions
14. Should support multiple types of users and different roles like Postgres
15. We need a UI client as well as database engine and all the above.
16. We also need native database drivers for almost all the languages including Python, Rust, Java, Javascript, C, C++, Perl, Typescript, Deno with high performance - THIS IS A MUST.
17. We also should be able to run this database natively on a local machine for smaller volumes.

Please follow SOLID principles and OOPS so that it is extensible as much as possible. Create reusable functions as much as possible.
UI Client can be separate project if needed and database engine should be separate.


None of the above requirements should be skipped. Please prepare a design document (create a file polap-db-design.md) and also workstructure (create a file polap-ws.md) in reference folder. The documents should contain every single detail, architecture diagram, design aspects, database driver details detailing every aspect very clearly as if we are explaning to a fresher out of the college.

Also create a high level README.md file with the summary of all the above in the root folder of polap-db.