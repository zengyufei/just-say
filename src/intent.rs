use serde::{Deserialize, Serialize};

const INTENT_SYSTEM_PROMPT: &str = r#"你是 JustSay 的语音意图分流器。你的任务是判断用户语音识别文本应该被当作普通输入文本，还是应该触发一个安全内置动作。

你必须非常保守：
1. 如果用户像是在写消息、写文章、写 prompt、记录笔记、输入一段话、让你优化文本、让你生成内容，必须返回 text_input。
2. 只有用户明确表达“搜索”“查一下”“查询”“打开”“启动”“访问”“进入网站”“打开设置”“打开日志”等动作时，才返回动作。
3. 模糊、不确定、可能只是要输入到当前文本框的内容，一律返回 text_input。
4. 不要执行或建议高风险动作，不要返回命令行、脚本、文件删除、系统控制、注册表修改等动作；这些返回 unsupported。
5. 只输出 JSON，不要输出 Markdown、解释、代码块或额外文字。

允许的 JSON：
{"intent":"text_input"}
{"intent":"web_search","query":"搜索关键词"}
{"intent":"open_url","url":"https://example.com"}
{"intent":"open_app","app_name":"notepad"}
{"intent":"open_settings"}
{"intent":"open_logs"}
{"intent":"unsupported","reason":"简短原因"}

字段要求：
- web_search.query 必须是用户要搜索或查询的内容。
- open_url.url 必须是网址或域名；如果用户只说域名，也可以返回域名。
- open_app.app_name 只能是应用名称，不要包含参数、路径、shell 片段或命令行。
- 如果用户说“打开设置”或“打开日志”，默认指 JustSay 的设置或日志，除非上下文明确是其他软件。
"#;

#[derive(Clone, Debug)]
pub struct IntentRouter {
    pub api_base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Clone, Debug)]
pub struct IntentOutcome {
    pub decision: IntentDecision,
    pub raw_json: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum IntentDecision {
    TextInput,
    WebSearch { query: String },
    OpenUrl { url: String },
    OpenApp { app_name: String },
    OpenSettings,
    OpenLogs,
    Unsupported { reason: Option<String> },
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    temperature: f32,
    messages: Vec<ChatMessage<'a>>,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: String,
}

impl IntentRouter {
    pub async fn decide(&self, text: &str) -> anyhow::Result<IntentOutcome> {
        if self.api_key.trim().is_empty() {
            anyhow::bail!("LLM API key is not configured");
        }
        let url = format!(
            "{}/chat/completions",
            self.api_base_url.trim_end_matches('/')
        );
        let body = ChatRequest {
            model: &self.model,
            temperature: 0.0,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: INTENT_SYSTEM_PROMPT,
                },
                ChatMessage {
                    role: "user",
                    content: text,
                },
            ],
        };
        let client = reqwest::Client::new();
        let response = client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let raw = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Intent API failed with {status}: {raw}");
        }
        let parsed: ChatResponse = serde_json::from_str(&raw)?;
        let Some(choice) = parsed.choices.into_iter().next() else {
            anyhow::bail!("Intent API returned no choices");
        };
        let content = choice.message.content.trim().to_string();
        let json = extract_json_object(&content)?;
        let decision: IntentDecision = serde_json::from_str(&json)?;
        Ok(IntentOutcome {
            decision: decision.normalized(),
            raw_json: json,
        })
    }
}

impl IntentDecision {
    pub fn is_text_input(&self) -> bool {
        matches!(self, Self::TextInput | Self::Unsupported { .. })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::TextInput => "text_input",
            Self::WebSearch { .. } => "web_search",
            Self::OpenUrl { .. } => "open_url",
            Self::OpenApp { .. } => "open_app",
            Self::OpenSettings => "open_settings",
            Self::OpenLogs => "open_logs",
            Self::Unsupported { .. } => "unsupported",
        }
    }

    fn normalized(self) -> Self {
        match self {
            Self::WebSearch { query } if query.trim().is_empty() => Self::TextInput,
            Self::WebSearch { query } => Self::WebSearch {
                query: query.trim().to_string(),
            },
            Self::OpenUrl { url } if url.trim().is_empty() => Self::TextInput,
            Self::OpenUrl { url } => Self::OpenUrl {
                url: url.trim().to_string(),
            },
            Self::OpenApp { app_name } if app_name.trim().is_empty() => Self::TextInput,
            Self::OpenApp { app_name } => Self::OpenApp {
                app_name: app_name.trim().to_string(),
            },
            other => other,
        }
    }
}

fn extract_json_object(value: &str) -> anyhow::Result<String> {
    let trimmed = value
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Ok(trimmed.to_string());
    }
    let Some(start) = trimmed.find('{') else {
        anyhow::bail!("intent response contains no JSON object");
    };
    let Some(end) = trimmed.rfind('}') else {
        anyhow::bail!("intent response contains no complete JSON object");
    };
    Ok(trimmed[start..=end].to_string())
}
