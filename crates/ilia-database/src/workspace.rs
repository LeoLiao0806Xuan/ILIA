use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{DatabaseError, WORKSPACE_SCHEMA_VERSION};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub sequence_number: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewSavedEvidence {
    pub stable_evidence_key: String,
    pub title: String,
    pub citation_label: String,
    pub text_snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedEvidence {
    pub id: String,
    pub project_id: Option<String>,
    pub stable_evidence_key: String,
    pub title: String,
    pub citation_label: String,
    pub text_snapshot: String,
    pub source_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    pub project_id: Option<String>,
    pub message_id: Option<String>,
    pub evidence_id: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoSection {
    pub id: String,
    pub project_id: String,
    pub heading: String,
    pub body: String,
    pub sequence_number: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceExport {
    pub project: Project,
    pub conversations: Vec<Conversation>,
    pub messages: Vec<Message>,
    pub evidence: Vec<SavedEvidence>,
    pub notes: Vec<Note>,
    pub memo_sections: Vec<MemoSection>,
}

pub struct WorkspaceDatabase {
    connection: Connection,
}

impl WorkspaceDatabase {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version = connection
            .query_row("SELECT max(version) FROM schema_migrations", [], |row| {
                row.get::<_, Option<i64>>(0)
            })?
            .unwrap_or(0);
        if version != WORKSPACE_SCHEMA_VERSION {
            return Err(DatabaseError::UnsupportedWritableSchema {
                database: "workspace",
                found: version,
                supported: WORKSPACE_SCHEMA_VERSION,
            });
        }
        Ok(Self { connection })
    }

    pub fn create_project(
        &mut self,
        title: &str,
        description: &str,
        tags: &[String],
    ) -> Result<Project, DatabaseError> {
        let id = Uuid::new_v4().to_string();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO projects(id,title,description) VALUES (?1,?2,?3)",
            params![id, title.trim(), description],
        )?;
        for tag in normalized_tags(tags) {
            let tag_id = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO tags(id,name) VALUES (?1,?2) ON CONFLICT(name) DO NOTHING",
                params![tag_id, tag],
            )?;
            transaction.execute(
                "INSERT INTO project_tags(project_id,tag_id) SELECT ?1,id FROM tags WHERE name=?2",
                params![id, tag],
            )?;
        }
        transaction.commit()?;
        self.project(&id)?.ok_or(DatabaseError::InvalidSchema)
    }

    pub fn update_project(
        &mut self,
        id: &str,
        title: &str,
        description: &str,
        tags: &[String],
    ) -> Result<Project, DatabaseError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE projects SET title=?2,description=?3,updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![id, title.trim(), description],
        )?;
        transaction.execute("DELETE FROM project_tags WHERE project_id=?1", [id])?;
        for tag in normalized_tags(tags) {
            let tag_id = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO tags(id,name) VALUES (?1,?2) ON CONFLICT(name) DO NOTHING",
                params![tag_id, tag],
            )?;
            transaction.execute(
                "INSERT INTO project_tags(project_id,tag_id) SELECT ?1,id FROM tags WHERE name=?2",
                params![id, tag],
            )?;
        }
        transaction.commit()?;
        self.project(id)?.ok_or(DatabaseError::InvalidSchema)
    }

    pub fn delete_project(&self, id: &str) -> Result<bool, DatabaseError> {
        Ok(self
            .connection
            .execute("DELETE FROM projects WHERE id=?1", [id])?
            == 1)
    }

    pub fn projects(&self) -> Result<Vec<Project>, DatabaseError> {
        let mut statement = self
            .connection
            .prepare("SELECT id,title,description FROM projects ORDER BY updated_at DESC,id")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, title, description) = row?;
            Ok(Project {
                tags: self.tags(&id)?,
                id,
                title,
                description,
            })
        })
        .collect()
    }

    pub fn create_conversation(
        &self,
        project_id: Option<&str>,
        title: &str,
    ) -> Result<Conversation, DatabaseError> {
        let value = Conversation {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.map(str::to_owned),
            title: title.to_owned(),
        };
        self.connection.execute(
            "INSERT INTO conversations(id,project_id,title) VALUES (?1,?2,?3)",
            params![value.id, value.project_id, value.title],
        )?;
        Ok(value)
    }

    pub fn append_message(
        &mut self,
        conversation_id: &str,
        role: &str,
        content: &str,
        citations: &[(i64, String, String)],
    ) -> Result<Message, DatabaseError> {
        let transaction = self.connection.transaction()?;
        let sequence_number: i64 = transaction.query_row(
            "SELECT COALESCE(max(sequence_number),0)+1 FROM messages WHERE conversation_id=?1",
            [conversation_id],
            |row| row.get(0),
        )?;
        let value = Message {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation_id.to_owned(),
            role: role.to_owned(),
            content: content.to_owned(),
            sequence_number,
        };
        transaction.execute("INSERT INTO messages(id,conversation_id,role,content,sequence_number) VALUES (?1,?2,?3,?4,?5)", params![value.id, value.conversation_id, value.role, value.content, value.sequence_number])?;
        for (number, key, label) in citations {
            transaction.execute("INSERT INTO message_citations(message_id,evidence_number,stable_evidence_key,citation_label) VALUES (?1,?2,?3,?4)", params![value.id, number, key, label])?;
        }
        transaction.execute(
            "UPDATE conversations SET updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            [conversation_id],
        )?;
        transaction.commit()?;
        Ok(value)
    }

    pub fn save_evidence(
        &self,
        project_id: Option<&str>,
        evidence: &NewSavedEvidence,
    ) -> Result<SavedEvidence, DatabaseError> {
        let value = SavedEvidence {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.map(str::to_owned),
            stable_evidence_key: evidence.stable_evidence_key.clone(),
            title: evidence.title.clone(),
            citation_label: evidence.citation_label.clone(),
            text_snapshot: evidence.text_snapshot.clone(),
            source_status: "available".to_owned(),
        };
        self.connection.execute("INSERT INTO saved_evidence(id,project_id,stable_evidence_key,title,citation_label,text_snapshot) VALUES (?1,?2,?3,?4,?5,?6)", params![value.id,value.project_id,value.stable_evidence_key,value.title,value.citation_label,value.text_snapshot])?;
        Ok(value)
    }

    pub fn mark_source_deleted(&self, stable_key: &str) -> Result<usize, DatabaseError> {
        Ok(self.connection.execute(
            "UPDATE saved_evidence SET source_status='deleted' WHERE stable_evidence_key=?1",
            [stable_key],
        )?)
    }

    pub fn upsert_note(
        &self,
        id: Option<&str>,
        project_id: Option<&str>,
        message_id: Option<&str>,
        evidence_id: Option<&str>,
        body: &str,
    ) -> Result<Note, DatabaseError> {
        let id = id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        self.connection.execute(
            "INSERT INTO notes(id,project_id,message_id,evidence_id,body) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,message_id=excluded.message_id,evidence_id=excluded.evidence_id,body=excluded.body,updated_at=CURRENT_TIMESTAMP",
            params![id,project_id,message_id,evidence_id,body],
        )?;
        Ok(Note {
            id,
            project_id: project_id.map(str::to_owned),
            message_id: message_id.map(str::to_owned),
            evidence_id: evidence_id.map(str::to_owned),
            body: body.to_owned(),
        })
    }

    pub fn replace_memo_sections(
        &mut self,
        project_id: &str,
        sections: &[(String, String)],
    ) -> Result<Vec<MemoSection>, DatabaseError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM memo_sections WHERE project_id=?1",
            [project_id],
        )?;
        let mut values = Vec::new();
        for (index, (heading, body)) in sections.iter().enumerate() {
            let value = MemoSection {
                id: Uuid::new_v4().to_string(),
                project_id: project_id.to_owned(),
                heading: heading.clone(),
                body: body.clone(),
                sequence_number: index as i64 + 1,
            };
            transaction.execute("INSERT INTO memo_sections(id,project_id,heading,body,sequence_number) VALUES (?1,?2,?3,?4,?5)", params![value.id,value.project_id,value.heading,value.body,value.sequence_number])?;
            values.push(value);
        }
        transaction.commit()?;
        Ok(values)
    }

    pub fn snapshot(&self, project_id: &str) -> Result<WorkspaceExport, DatabaseError> {
        let project = self
            .project(project_id)?
            .ok_or(DatabaseError::InvalidSchema)?;
        Ok(WorkspaceExport {
            project,
            conversations: self.query_conversations(project_id)?,
            messages: self.query_messages(project_id)?,
            evidence: self.query_evidence(project_id)?,
            notes: self.query_notes(project_id)?,
            memo_sections: self.query_memo(project_id)?,
        })
    }

    pub fn export_markdown(&self, project_id: &str) -> Result<String, DatabaseError> {
        let data = self.snapshot(project_id)?;
        let mut out = format!("# {}\n\n{}\n", data.project.title, data.project.description);
        for section in data.memo_sections {
            out.push_str(&format!("\n## {}\n\n{}\n", section.heading, section.body));
        }
        if !data.notes.is_empty() {
            out.push_str("\n## 笔记\n");
            for note in data.notes {
                out.push_str(&format!("\n- {}\n", note.body.replace('\n', "\n  ")));
            }
        }
        if !data.evidence.is_empty() {
            out.push_str("\n## 证据快照\n");
            for evidence in data.evidence {
                out.push_str(&format!(
                    "\n### {} — {}\n\n{}\n\n稳定键：`{}`；来源状态：{}\n",
                    evidence.title,
                    evidence.citation_label,
                    evidence.text_snapshot,
                    evidence.stable_evidence_key,
                    evidence.source_status
                ));
            }
        }
        Ok(out)
    }

    pub fn export_html(&self, project_id: &str) -> Result<String, DatabaseError> {
        let data = self.snapshot(project_id)?;
        let mut body = format!(
            "<h1>{}</h1><p>{}</p>",
            escape(&data.project.title),
            escape(&data.project.description)
        );
        for section in data.memo_sections {
            body.push_str(&format!(
                "<section><h2>{}</h2><p>{}</p></section>",
                escape(&section.heading),
                escape(&section.body).replace('\n', "<br>")
            ));
        }
        if !data.notes.is_empty() {
            body.push_str("<section><h2>笔记</h2><ul>");
            for note in data.notes {
                body.push_str(&format!(
                    "<li>{}</li>",
                    escape(&note.body).replace('\n', "<br>")
                ));
            }
            body.push_str("</ul></section>");
        }
        if !data.evidence.is_empty() {
            body.push_str("<section><h2>证据快照</h2>");
            for evidence in data.evidence {
                body.push_str(&format!("<article><h3>{} — {}</h3><blockquote>{}</blockquote><p><code>{}</code>；来源状态：{}</p></article>", escape(&evidence.title), escape(&evidence.citation_label), escape(&evidence.text_snapshot).replace('\n', "<br>"), escape(&evidence.stable_evidence_key), escape(&evidence.source_status)));
            }
            body.push_str("</section>");
        }
        Ok(format!(
            "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>body{{max-width:54rem;margin:2rem auto;padding:0 1rem;font:16px/1.7 system-ui;color:#18211d}}blockquote{{border-left:3px solid #2f6d57;margin:1rem 0;padding:.5rem 1rem;background:#f4f6f4}}code{{overflow-wrap:anywhere}}</style><body>{}</body></html>",
            escape(&data.project.title),
            body
        ))
    }

    fn project(&self, id: &str) -> Result<Option<Project>, DatabaseError> {
        let row = self
            .connection
            .query_row(
                "SELECT id,title,description FROM projects WHERE id=?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(id, title, description)| {
            Ok(Project {
                tags: self.tags(&id)?,
                id,
                title,
                description,
            })
        })
        .transpose()
    }
    fn tags(&self, id: &str) -> Result<Vec<String>, DatabaseError> {
        query_strings(
            &self.connection,
            "SELECT t.name FROM tags t JOIN project_tags pt ON pt.tag_id=t.id WHERE pt.project_id=?1 ORDER BY t.name",
            id,
        )
    }
    fn query_conversations(&self, id: &str) -> Result<Vec<Conversation>, DatabaseError> {
        let mut s=self.connection.prepare("SELECT id,project_id,title FROM conversations WHERE project_id=?1 ORDER BY created_at,id")?;
        Ok(s.query_map([id], |r| {
            Ok(Conversation {
                id: r.get(0)?,
                project_id: r.get(1)?,
                title: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?)
    }
    fn query_messages(&self, id: &str) -> Result<Vec<Message>, DatabaseError> {
        let mut s=self.connection.prepare("SELECT m.id,m.conversation_id,m.role,m.content,m.sequence_number FROM messages m JOIN conversations c ON c.id=m.conversation_id WHERE c.project_id=?1 ORDER BY c.created_at,c.id,m.sequence_number")?;
        Ok(s.query_map([id], |r| {
            Ok(Message {
                id: r.get(0)?,
                conversation_id: r.get(1)?,
                role: r.get(2)?,
                content: r.get(3)?,
                sequence_number: r.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?)
    }
    fn query_evidence(&self, id: &str) -> Result<Vec<SavedEvidence>, DatabaseError> {
        let mut s=self.connection.prepare("SELECT id,project_id,stable_evidence_key,title,citation_label,text_snapshot,source_status FROM saved_evidence WHERE project_id=?1 ORDER BY created_at,id")?;
        Ok(s.query_map([id], |r| {
            Ok(SavedEvidence {
                id: r.get(0)?,
                project_id: r.get(1)?,
                stable_evidence_key: r.get(2)?,
                title: r.get(3)?,
                citation_label: r.get(4)?,
                text_snapshot: r.get(5)?,
                source_status: r.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?)
    }
    fn query_notes(&self, id: &str) -> Result<Vec<Note>, DatabaseError> {
        let mut s=self.connection.prepare("SELECT id,project_id,message_id,evidence_id,body FROM notes WHERE project_id=?1 ORDER BY updated_at,id")?;
        Ok(s.query_map([id], |r| {
            Ok(Note {
                id: r.get(0)?,
                project_id: r.get(1)?,
                message_id: r.get(2)?,
                evidence_id: r.get(3)?,
                body: r.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?)
    }
    fn query_memo(&self, id: &str) -> Result<Vec<MemoSection>, DatabaseError> {
        let mut s=self.connection.prepare("SELECT id,project_id,heading,body,sequence_number FROM memo_sections WHERE project_id=?1 ORDER BY sequence_number,id")?;
        Ok(s.query_map([id], |r| {
            Ok(MemoSection {
                id: r.get(0)?,
                project_id: r.get(1)?,
                heading: r.get(2)?,
                body: r.get(3)?,
                sequence_number: r.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?)
    }
}

fn normalized_tags(tags: &[String]) -> Vec<String> {
    let mut tags = tags
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    tags.sort();
    tags.dedup();
    tags
}
fn query_strings(
    connection: &Connection,
    sql: &str,
    id: &str,
) -> Result<Vec<String>, DatabaseError> {
    let mut s = connection.prepare(sql)?;
    Ok(s.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?)
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApplicationDatabasePaths;
    use std::fs;

    fn database() -> (std::path::PathBuf, WorkspaceDatabase) {
        let root = std::env::temp_dir().join(format!("ilia-workspace-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let core = root.join("core.sqlite");
        let c = Connection::open(&core).unwrap();
        c.execute_batch("CREATE TABLE schema_metadata(schema_version TEXT PRIMARY KEY,applied_at TEXT);INSERT INTO schema_metadata VALUES('001',CURRENT_TIMESTAMP);").unwrap();
        drop(c);
        let paths = ApplicationDatabasePaths::initialize(core, root.join("data")).unwrap();
        let db = WorkspaceDatabase::open(paths.workspace).unwrap();
        (root, db)
    }

    #[test]
    fn persists_crud_and_cascades() {
        let (root, mut db) = database();
        let project = db
            .create_project("海洋法", "研究", &["海洋".into(), "海洋".into()])
            .unwrap();
        let conversation = db.create_conversation(Some(&project.id), "会话").unwrap();
        db.append_message(&conversation.id, "user", "问题", &[])
            .unwrap();
        let evidence = db
            .save_evidence(
                Some(&project.id),
                &NewSavedEvidence {
                    stable_evidence_key: "core:unclos:a3".into(),
                    title: "UNCLOS".into(),
                    citation_label: "Article 3".into(),
                    text_snapshot: "<script>x</script>正文".into(),
                },
            )
            .unwrap();
        db.upsert_note(
            None,
            Some(&project.id),
            None,
            Some(&evidence.id),
            "<img onerror=x>",
        )
        .unwrap();
        db.replace_memo_sections(&project.id, &[("结论".into(), "内容".into())])
            .unwrap();
        drop(db);
        let db = WorkspaceDatabase::open(root.join("data/workspace.sqlite")).unwrap();
        assert_eq!(db.snapshot(&project.id).unwrap().messages.len(), 1);
        let html = db.export_html(&project.id).unwrap();
        assert!(!html.contains("<script>x</script>"));
        assert!(!html.contains("<img onerror"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(db.delete_project(&project.id).unwrap());
        assert!(db.snapshot(&project.id).is_err());
        let orphan: i64 = db
            .connection
            .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(orphan, 0);
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }
}
