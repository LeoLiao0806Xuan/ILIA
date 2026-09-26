# DOCX fixture

The Rust import test creates a minimal DOCX ZIP in memory from `word/document.xml`, then creates a second archive containing `word/vbaProject.bin` to verify rejection of active content. Keeping the fixture source textual makes the security case reviewable and reproducible.
