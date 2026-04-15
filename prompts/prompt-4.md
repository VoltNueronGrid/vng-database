===============================================================================
Please create github instructions, skills, agents for:
1. code review
2. gaps analysis
3. plan update / tracker update
4. security
5. testing - unit testing, mutation testing, integration testing, code coverage for 90% minimum
6. we need to execute and develop using multiple agents so that development is faster
7. ensure all the gaps are complete
8. follow SOLID princples
9. reusable code as much as possible
===============================================================================I want one single agent which will call all the other existing agents / skills based on the requirement or as per need. In this multi-agent is must to parallelize the work. 
===============================================================================
Please start with "4.3 MCP track — production-ready server capability" from #file:sub-tasks.md and complete without any gaps.

Please test it after completion - Write unit tests as well.

Follow Github copilot instructions.

Update sub-tasks.md after completing the tasks and  commit the code and push to origin
===============================================================================
Create a very detailed and comprehensive README-MCP.md with all the details on MCP and steps to configure and run with multiple examples, steps to configure and run in VSCode, Cursor and Claude and CLI.

Need comprehensive examples as well.

Include all the details.
===============================================================================
Can you add examples for adding MCP in VSCode or other IDEs like Cursor if it is from Docker container (local) or Docker (hosted on cloud) or any hosted or if DB is running from cloud ? In these cases how to add the MCP and connect to that MCP ?
===============================================================================
Can we add more functionality to MCP with the below:

I want to also add :
create table, function,, views or any database object that we support etc, create a ERD diagram for the given tables, or given schema, import or export data (from/to CSV, parquet, Blob, Webdav, FTP) - with additional key

drop table, functions, views, or any database object that we support etc - with additional key


===============================================================================

Add some more functionality to MCP with admin key:

1. Should be able to give me the topology of connected cluster like how many nodes, how many active, how manyare passive, configuration of those nodes like total CPU and RAM of each node, use CPU and RAM of each node, how many are active, how many are dead (if any), number of active / passive sessions, how many live transactions , total transactions, should be able to commit or roll back transactions, should be able to kill any locks or dead-locks.

should be able to manage the cluster by adding a new node and make it join the cluster to share the the load or remove the node to save infra / cost (automatically all the live transactions or any ongoing transactions should move to existing nodes without any data loss)

Please change / refactor the actual database code if needed

Update README.md after all the above tasks are completed with comprehensive examples.