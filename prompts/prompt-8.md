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