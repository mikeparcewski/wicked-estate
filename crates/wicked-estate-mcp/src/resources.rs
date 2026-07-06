//! Bundled skill resources surfaced as MCP resources (`skill://` scheme).
//! Embedded at compile time so they travel with the binary.

use serde_json::{Value, json};
use std::sync::OnceLock;

pub struct McpResource {
    pub uri: String,
    pub content: &'static str,
}

impl McpResource {
    pub fn name(&self) -> String {
        self.uri.trim_start_matches("skill://").to_string()
    }
}

static BUNDLED_SKILLS: OnceLock<Vec<McpResource>> = OnceLock::new();

fn build_skill_list() -> Vec<McpResource> {
    vec![
        McpResource {
            uri: "skill://codebase-expedition/SKILL.md".to_string(),
            content: include_str!("../../wicked-estate-memory/skills/codebase-expedition/SKILL.md"),
        },
        McpResource {
            uri: "skill://knowledge-ingest/SKILL.md".to_string(),
            content: include_str!("../../wicked-estate-knowledge/skills/knowledge-ingest/SKILL.md"),
        },
        McpResource {
            uri: "skill://ontology-expedition/SKILL.md".to_string(),
            content: include_str!(
                "../../wicked-estate-knowledge/skills/ontology-expedition/SKILL.md"
            ),
        },
        McpResource {
            uri: "skill://knowledge-curation/SKILL.md".to_string(),
            content: include_str!(
                "../../wicked-estate-knowledge/skills/knowledge-curation/SKILL.md"
            ),
        },
        McpResource {
            uri: "skill://cited-answer/SKILL.md".to_string(),
            content: include_str!("../../wicked-estate-knowledge/skills/cited-answer/SKILL.md"),
        },
        McpResource {
            uri: "skill://gap-hunting/SKILL.md".to_string(),
            content: include_str!("../../wicked-estate-knowledge/skills/gap-hunting/SKILL.md"),
        },
    ]
}

pub fn bundled_skills() -> &'static Vec<McpResource> {
    BUNDLED_SKILLS.get_or_init(|| {
        let skills = build_skill_list();
        let mut uris: Vec<&str> = skills.iter().map(|s| s.uri.as_str()).collect();
        uris.sort_unstable();
        for i in 1..uris.len() {
            assert_ne!(
                uris[i - 1],
                uris[i],
                "BUG: duplicate skill URI: {}",
                uris[i]
            );
        }
        skills
    })
}

pub fn resources_list(id: &Value) -> Value {
    let skills = bundled_skills();
    let items: Vec<Value> = skills
        .iter()
        .map(|s| {
            json!({
                "uri":      s.uri,
                "name":     s.name(),
                "mimeType": "text/markdown"
            })
        })
        .collect();
    json!({"jsonrpc":"2.0","id":id,"result":{"resources": items}})
}

pub fn resources_read(id: &Value, uri: &str) -> Value {
    match bundled_skills().iter().find(|s| s.uri == uri) {
        Some(s) => json!({"jsonrpc":"2.0","id":id,"result":{
            "contents": [{"uri": uri, "text": s.content}]
        }}),
        None => {
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":format!("resource not found: {uri}")}})
        }
    }
}

pub fn prompts_list(id: &Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":{"prompts":[{
        "name": "expedition",
        "description": "Hotspot-first codebase exploration: RankHotspots → TraverseGraph → FetchContent",
        "arguments": [
            {"name": "repo_path", "description": "Path to the indexed repo", "required": true}
        ]
    }]}})
}

pub fn prompts_get(id: &Value, name: &str) -> Value {
    if name != "expedition" {
        return json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":format!("prompt not found: {name}")}});
    }
    let skill_text = include_str!("../../wicked-estate-memory/skills/codebase-expedition/SKILL.md");
    json!({"jsonrpc":"2.0","id":id,"result":{
        "messages": [{"role":"user","content":{"type":"text","text":skill_text}}]
    }})
}
