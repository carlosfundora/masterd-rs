## 2024-05-27 - [Avoid Unnecessary Clones]
**Learning:** The pipeline deduplication was cloning `doc_id` and the `candidate` unnecessarily. For string maps, we can use the entry API correctly or consume the value.
**Action:** Remove unnecessary clone calls when taking ownership.
