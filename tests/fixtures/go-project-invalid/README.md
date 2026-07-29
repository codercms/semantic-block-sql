# Invalid Go integration fixture

This project contains one otherwise formatable Go file and one Go file with malformed embedded PostgreSQL. The integration test snapshots the project, runs `semblock fmt`, expects exit class 3, and verifies that no file was rewritten.
