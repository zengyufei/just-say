use crate::config::Language;
use serde::{Deserialize, Serialize};

const UNDERSTANDING_SYSTEM_PROMPT: &str = r#"你是 JustSay 的语义理解引擎。JustSay 的边界是“快速语音输入法助手”，不是通用 Agent，也不是专业秘书。输入法是本职工作；助手能力只用于理解口语、输出成稿、整理、修复和少量轻动作。

默认策略：
1. 优先把用户的话理解为要输入到当前输入框的内容。模糊时必须走文本输出，不要触发动作。
2. 你可以像一个克制、贴身、懂用户语气的秘书一样，把口语整理成用户真正想输入的文本。
3. 你不能做专业决策、复杂自动化、系统控制、文件修改、命令执行、跨软件操作流程、账号/隐私/高风险动作。
4. 轻动作只包括搜索、打开 URL、打开本机软件、打开 JustSay 设置、打开 JustSay 日志。除此之外返回 unsupported 或文本输出。

语义类型：
1. 用户直接口述要输入的文字，返回 dictation，并把 final_text 整理为可直接粘贴的文本。
2. 用户是在纠正语音识别错误、修复错字、恢复真实表达，例如“不是 X，是 Y”“刚才识别错了”“帮我修一下这句话”，返回 repair。
3. 用户说“这段话/刚才那个/上一条帮我改正式一点/更短一点/润色一下”，结合 recent_context 返回 rewrite。
4. 用户说“我要写/帮我写/生成/起草/拟一封/写一段/写一条/回复一封”等写作任务，返回 compose，并在 final_text 输出成稿，不要保留“我要写/内容是”等元指令。
5. 用户是在记录备忘、灵感、待办、会议纪要、临时想法，例如“记一下/帮我记个事/备忘一下/今天想到”，返回 note，并把 final_text 整理成简洁记录。
6. 用户明确说“搜索/查一下/查询”，返回 web_search，action_target 是搜索关键词。
7. 用户明确说“打开/访问/进入”某个网站或 URL，返回 open_url，action_target 是 URL 或域名。
8. 用户明确说“打开/启动”某个本机软件，返回 open_app，action_target 只放应用名称，不要放参数、路径、脚本或命令行。
9. 用户说打开 JustSay 设置或日志，返回 open_settings 或 open_logs。
10. 高风险、系统控制、文件删除修改、命令行、脚本、注册表、隐私危险动作、专业任务代理执行返回 unsupported。
11. 用户只是说“帮我查一下这句话有没有问题/这个表达对不对/把这句话整理一下”，这属于文本修复或改写，不属于网页搜索。

confidence 是 0 到 100 的整数，表示你对 kind、final_text/action_target 是否准确表达用户真实意图的信心。低于 85 表示需要二次修复或保守回退。

只输出 JSON，不要输出 Markdown、解释、代码块或额外文字。格式：
{"kind":"dictation","confidence":90,"intent_summary":"简短中文说明","final_text":"可粘贴文本","action_target":"","reason":"简短原因"}

kind 只能是：dictation、repair、rewrite、compose、note、web_search、open_url、open_app、open_settings、open_logs、unsupported。
"#;

#[derive(Clone, Debug, Serialize)]
pub struct UnderstandingRequest {
    pub raw_text: String,
    pub language: Language,
    pub foreground_window_title: String,
    pub recent_context: Vec<RecentInteraction>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecentInteraction {
    pub raw_text: String,
    pub kind: UnderstandingKind,
    pub final_text: String,
    pub action_result: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnderstandingKind {
    Dictation,
    Repair,
    Compose,
    Rewrite,
    Note,
    WebSearch,
    OpenUrl,
    OpenApp,
    OpenSettings,
    OpenLogs,
    Unsupported,
}

impl UnderstandingKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Repair => "repair",
            Self::Compose => "compose",
            Self::Rewrite => "rewrite",
            Self::Note => "note",
            Self::WebSearch => "web_search",
            Self::OpenUrl => "open_url",
            Self::OpenApp => "open_app",
            Self::OpenSettings => "open_settings",
            Self::OpenLogs => "open_logs",
            Self::Unsupported => "unsupported",
        }
    }

    pub fn is_text_output(self) -> bool {
        matches!(
            self,
            Self::Dictation | Self::Repair | Self::Compose | Self::Rewrite | Self::Note
        )
    }

    pub fn is_action(self) -> bool {
        matches!(
            self,
            Self::WebSearch | Self::OpenUrl | Self::OpenApp | Self::OpenSettings | Self::OpenLogs
        )
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct UnderstandingDecision {
    pub kind: UnderstandingKind,
    pub confidence: u8,
    pub intent_summary: String,
    pub final_text: String,
    pub action_target: String,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct UnderstandingOutcome {
    pub decision: UnderstandingDecision,
    pub raw_json: String,
}

#[derive(Clone, Debug)]
pub struct UnderstandingRouter {
    pub api_base_url: String,
    pub api_key: String,
    pub model: String,
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

#[derive(Debug, Deserialize)]
struct WireDecision {
    kind: UnderstandingKind,
    #[serde(default)]
    confidence: Option<serde_json::Value>,
    #[serde(default)]
    intent_summary: String,
    #[serde(default)]
    final_text: String,
    #[serde(default)]
    action_target: String,
    #[serde(default)]
    reason: String,
}

impl UnderstandingRouter {
    pub async fn understand(
        &self,
        request: &UnderstandingRequest,
    ) -> anyhow::Result<UnderstandingOutcome> {
        if self.api_key.trim().is_empty() {
            anyhow::bail!("LLM API key is not configured");
        }
        let request_json = serde_json::to_string_pretty(request)?;
        let content = self.call_chat(&request_json).await?;
        parse_understanding_json(&content, &request.raw_text)
    }

    async fn call_chat(&self, user_content: &str) -> anyhow::Result<String> {
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
                    content: UNDERSTANDING_SYSTEM_PROMPT,
                },
                ChatMessage {
                    role: "user",
                    content: user_content,
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
            anyhow::bail!("Understanding API failed with {status}: {raw}");
        }
        let parsed: ChatResponse = serde_json::from_str(&raw)?;
        let Some(choice) = parsed.choices.into_iter().next() else {
            anyhow::bail!("Understanding API returned no choices");
        };
        Ok(choice.message.content.trim().to_string())
    }
}

pub fn parse_understanding_json(
    value: &str,
    raw_text: &str,
) -> anyhow::Result<UnderstandingOutcome> {
    let json = extract_json_object(value)?;
    let wire: WireDecision = serde_json::from_str(&json)?;
    let mut decision = UnderstandingDecision {
        kind: wire.kind,
        confidence: parse_confidence(wire.confidence.as_ref()),
        intent_summary: wire.intent_summary.trim().to_string(),
        final_text: wire.final_text.trim().to_string(),
        action_target: wire.action_target.trim().to_string(),
        reason: wire.reason.trim().to_string(),
    };
    if decision.kind.is_text_output() && decision.final_text.is_empty() {
        decision.final_text = raw_text.trim().to_string();
        decision.confidence = decision.confidence.min(70);
    }
    if decision.kind.is_action() && decision.action_target.is_empty() {
        decision.kind = UnderstandingKind::Unsupported;
        decision.confidence = decision.confidence.min(50);
        decision.reason = "动作目标为空".to_string();
    }
    Ok(UnderstandingOutcome {
        decision,
        raw_json: json,
    })
}

fn parse_confidence(value: Option<&serde_json::Value>) -> u8 {
    let confidence = match value {
        Some(serde_json::Value::Number(number)) => number.as_f64().unwrap_or(0.0),
        Some(serde_json::Value::String(text)) => text.trim().parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    };
    confidence.clamp(0.0, 100.0).round() as u8
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
        anyhow::bail!("understanding response contains no JSON object");
    };
    let Some(end) = trimmed.rfind('}') else {
        anyhow::bail!("understanding response contains no complete JSON object");
    };
    Ok(trimmed[start..=end].to_string())
}

#[cfg(test)]
mod tests {
    use super::{parse_understanding_json, UnderstandingKind};

    #[test]
    fn parses_compose_understanding() {
        let outcome = parse_understanding_json(
            r#"{"kind":"compose","confidence":92,"intent_summary":"写邮件","final_text":"领导您好，关于昨天的任务，我今天可能无法完成。","action_target":"","reason":"明确写作任务"}"#,
            "我要写一封发给领导的邮件，内容是关于昨天的任务，我今天可能没有办法完成。",
        )
        .unwrap();
        assert_eq!(outcome.decision.kind, UnderstandingKind::Compose);
        assert!(!outcome.decision.final_text.contains("我要写"));
        assert!(!outcome.decision.final_text.contains("内容是"));
    }

    #[test]
    fn parses_open_app_understanding() {
        let outcome = parse_understanding_json(
            r#"{"kind":"open_app","confidence":"91","intent_summary":"打开软件","final_text":"","action_target":"记事本","reason":"明确打开应用"}"#,
            "帮我打开记事本",
        )
        .unwrap();
        assert_eq!(outcome.decision.kind, UnderstandingKind::OpenApp);
        assert_eq!(outcome.decision.action_target, "记事本");
    }

    #[test]
    fn parses_web_search_understanding() {
        let outcome = parse_understanding_json(
            r#"{"kind":"web_search","confidence":96,"intent_summary":"搜索资料","final_text":"","action_target":"Rust cpal 录音","reason":"明确搜索"}"#,
            "搜索 Rust cpal 录音",
        )
        .unwrap();
        assert_eq!(outcome.decision.kind, UnderstandingKind::WebSearch);
    }

    #[test]
    fn parses_rewrite_understanding() {
        let outcome = parse_understanding_json(
            r#"{"kind":"rewrite","confidence":88,"intent_summary":"改写上一条","final_text":"这是更正式的版本。","action_target":"","reason":"结合上下文改写"}"#,
            "这段话帮我改正式一点",
        )
        .unwrap();
        assert_eq!(outcome.decision.kind, UnderstandingKind::Rewrite);
    }

    #[test]
    fn parses_repair_understanding() {
        let outcome = parse_understanding_json(
            r#"{"kind":"repair","confidence":90,"intent_summary":"修复识别错误","final_text":"不是配森，是 Python。","action_target":"","reason":"明确纠错"}"#,
            "不是配森，是 Python",
        )
        .unwrap();
        assert_eq!(outcome.decision.kind, UnderstandingKind::Repair);
    }

    #[test]
    fn parses_note_understanding() {
        let outcome = parse_understanding_json(
            r#"{"kind":"note","confidence":89,"intent_summary":"记录备忘","final_text":"待办：明天上午跟进发票。","action_target":"","reason":"明确记录事项"}"#,
            "记一下，明天上午跟进发票",
        )
        .unwrap();
        assert_eq!(outcome.decision.kind, UnderstandingKind::Note);
    }

    #[test]
    fn empty_action_target_becomes_unsupported() {
        let outcome = parse_understanding_json(
            r#"{"kind":"open_app","confidence":90,"intent_summary":"打开软件","final_text":"","action_target":"","reason":"缺少目标"}"#,
            "打开那个",
        )
        .unwrap();
        assert_eq!(outcome.decision.kind, UnderstandingKind::Unsupported);
    }
}
