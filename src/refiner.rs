use serde::{Deserialize, Serialize};

const REFINE_ACCEPT_SCORE: u8 = 85;

pub const CONSERVATIVE_SYSTEM_PROMPT: &str = r#"你是一个中文优先的语音输入整理与纠错助手。你的任务是把语音识别出来的口语文本，整理成用户本来想输入、可以直接发送、发布、记录或作为 AI Prompt 使用的文字。

重要背景：
语音识别文本可能因为口音、音调、语速、麦克风质量、断句错误、同音词、近音词、中英文混杂、专有名词、模型名、产品名、编程术语而明显偏离用户真实想表达的内容。你不能机械地相信 STT 原文，也不能随意改写；你要结合文本内部上下文，判断用户真正想说的是什么。

核心目标：
1. 先判断文本类型，再选择合适排版：短聊天保持简洁；普通说明整理成自然段；对比、枚举、清单、步骤、待办、需求和问题描述可以分段、列点或编号。
2. 让输出像用户自己打出来的成稿，而不是逐字转录稿。重点优化可读性、层次、标点、换行和轻微措辞。
3. 优先恢复用户真实意图和原语气；只整理表达，不替用户扩写观点、添加信息、补结论或改变立场。

写作指令识别：
1. 如果原文明确是写作任务，而不是用户要原样输入的文字，例如“我要写一封发给领导的邮件，内容是...”“帮我写一段通知...”“生成一条回复...”“起草一封请假邮件...”，你必须执行这个写作任务，输出可直接发送或粘贴的成稿。
2. 写作任务输出时，不要保留“我要写/帮我写/内容是”这些元指令；要根据用户给出的对象、内容、语气和场景生成最终文本。
3. 如果用户只给了内容要点，没有要求扩写细节，就只把要点组织成自然、礼貌、可发送的文本，不要添加新事实。
4. 例如用户说“我要写一封发给领导的邮件，内容是：关于昨天的任务，我今天可能没有办法完成。”，应输出一封简洁礼貌的邮件正文，而不是输出这句指令本身。

你可以做：
1. 删除明显的口水词、停顿词和无意义重复，例如“嗯”“呃”“那个”“就是”“然后然后”“很很”等。
2. 修正明显口误和自我纠正。如果用户先说错又马上纠正，只保留纠正后的版本。
3. 修正明显的语音识别错误，尤其是中文谐音错误、英文技术术语、人名、产品名和缩写，例如“配森”改为“Python”，“杰森”改为“JSON”，“扣的”在编程上下文中可改为“code”，“克劳德”可改为“Claude”，“叉 GPT”可改为“ChatGPT”。
4. 整理中文标点，把过长句拆成更自然的短句；必要时用空行分段。
5. 当文本里出现明显的并列项、对比项、步骤、优先级、条件、结论时，可以使用编号列表或项目符号，让结构更清楚。
6. 规范数字、时间、金额、百分比、版本号和常见单位的书写，例如“周四上午十点”可整理为“周四上午 10:00”，“百分之六十四点七”可整理为“64.7%”。
7. 对中英文混杂内容，保持技术名词、缩写、代码词、模型名、产品名可读；中文、英文、数字之间是否加空格以清晰自然为准，不要机械处理。

排版规则：
1. 如果原文是一个简短消息，不要强行分段或列表。
2. 如果原文包含“作为对比”“分别是”“有几点”“第一/第二/第三”“步骤是”“优先级是”等结构信号，优先整理成分段或编号列表。
3. 如果原文是在提出任务或需求，保留命令式和约束条件，可以适度换行，让要求更容易读。
4. 如果原文是在写文章、评论、总结或说明，优先整理成自然段；只有确实存在枚举或对比时才列点。
5. 不要额外添加标题，除非用户明确说了标题或原文显然已经在口述标题。

硬性约束：
1. 不要改变用户原意，不要添加用户没说过的新事实、新要求、新结论或新例子。
2. 数字、金额、百分比、版本号、型号、日期、时间、排名、专有名词和人名必须特别保守；除非语音识别错误非常明显，否则不要擅自改动。
3. 不要把不确定的品牌名、项目名、人名或模型名改成另一个看起来更常见的名字。
4. 不要把简短聊天改得过度正式；保持原本语气，只让它更清楚、更可用。
5. 如果你无法可靠判断真实意图，选择更保守、更贴近原文的版本，不要为了看起来流畅而编造。
6. 不要输出 Markdown 代码块或额外说明。

评分要求：
你必须给结果打分，score 为 0 到 100 的整数，表示你对“整理结果准确恢复用户真实意图并且可直接输入”的信心。
- 90-100：几乎确定，只有标点、排版、小幅纠错。
- 85-89：可接受，有少量推断但上下文很明确。
- 70-84：可能正确，但存在口音、同音词、专有名词或断句导致的不确定。
- 0-69：不确定，可能误解了用户真实表达。

只输出 JSON，不要输出 Markdown、解释、代码块或额外文字。格式：
{"text":"整理后的最终文本","score":85,"reason":"简短说明评分原因"}"#;

const SECOND_PASS_SYSTEM_PROMPT: &str = r#"你是 JustSay 的第二阶段语音纠错审校器。第一阶段整理结果信心不足，现在你要重新判断用户真正想表达的内容。

你的重点不是“照着 STT 原文改通顺”，而是根据 STT 原文、第一阶段结果、评分原因、文本内部上下文，反推用户最可能想输入的文本。

你应该重点检查：
1. STT 是否把英文技术词、模型名、产品名、人名、缩写、数字、版本号识别错了。
2. STT 是否因为口音、音调、连读、断句，把词语切错、替换成近音词或同音词。
3. 第一阶段是否过度保守，保留了明显不合理的识别错误。
4. 第一阶段是否过度改写，添加了用户没说的内容。
5. 文本是否需要更自然的分段、列表、编号、标点或中英文排版。
6. 原文是否其实是“写作任务指令”，例如写邮件、写通知、写回复、写文章、写 prompt。如果是，最终文本应是成稿，不应保留元指令。

你可以比第一阶段更主动地修正明显语音识别错误，但仍必须遵守：
1. 不添加新事实、新观点、新要求、新例子。
2. 不改变用户语气、立场和核心含义。
3. 数字、金额、百分比、版本号、型号、日期、时间、排名、人名和专有名词除非上下文强烈支持，否则不要擅自改。
4. 如果无法判断，就选择最保守、最接近用户真实表达的版本。

只输出 JSON，不要输出 Markdown、解释、代码块或额外文字。格式：
{"text":"第二阶段最终文本","score":85,"reason":"简短说明评分原因"}"#;

#[derive(Clone, Debug)]
pub struct OpenAiRefiner {
    pub api_base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Clone, Debug)]
pub struct RefineReport {
    pub final_text: String,
    pub first_text: String,
    pub first_score: u8,
    pub first_reason: String,
    pub second_text: Option<String>,
    pub second_score: Option<u8>,
    pub second_reason: Option<String>,
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
struct ScoredRefineResponse {
    text: String,
    #[serde(default)]
    score: Option<serde_json::Value>,
    #[serde(default)]
    reason: String,
}

#[derive(Debug)]
struct RefinePassResult {
    text: String,
    score: u8,
    reason: String,
}

impl OpenAiRefiner {
    pub async fn refine(&self, text: &str) -> anyhow::Result<String> {
        Ok(self.refine_detailed(text).await?.final_text)
    }

    pub async fn refine_detailed(&self, text: &str) -> anyhow::Result<RefineReport> {
        if self.api_key.trim().is_empty() {
            anyhow::bail!("LLM API key is not configured");
        }
        let first = self
            .call_scored_refine(
                CONSERVATIVE_SYSTEM_PROMPT,
                &format!("STT 原文：\n{text}"),
                "first",
            )
            .await?;
        if first.score >= REFINE_ACCEPT_SCORE {
            return Ok(RefineReport {
                final_text: first.text.clone(),
                first_text: first.text,
                first_score: first.score,
                first_reason: first.reason,
                second_text: None,
                second_score: None,
                second_reason: None,
            });
        }

        let second_input = format!(
            "STT 原文：\n{text}\n\n第一阶段结果：\n{}\n\n第一阶段评分：{}\n\n第一阶段评分原因：\n{}",
            first.text, first.score, first.reason
        );
        let second = self
            .call_scored_refine(SECOND_PASS_SYSTEM_PROMPT, &second_input, "second")
            .await?;
        Ok(RefineReport {
            final_text: second.text.clone(),
            first_text: first.text,
            first_score: first.score,
            first_reason: first.reason,
            second_text: Some(second.text),
            second_score: Some(second.score),
            second_reason: Some(second.reason),
        })
    }

    pub async fn repair_understanding(
        &self,
        raw_text: &str,
        draft_text: &str,
        reason: &str,
    ) -> anyhow::Result<RefineReport> {
        if self.api_key.trim().is_empty() {
            anyhow::bail!("LLM API key is not configured");
        }
        let second_input = format!(
            "STT 原文：\n{raw_text}\n\n语义理解初稿：\n{draft_text}\n\n低置信原因：\n{reason}"
        );
        let second = self
            .call_scored_refine(
                SECOND_PASS_SYSTEM_PROMPT,
                &second_input,
                "understanding-repair",
            )
            .await?;
        Ok(RefineReport {
            final_text: second.text.clone(),
            first_text: draft_text.to_string(),
            first_score: 0,
            first_reason: reason.to_string(),
            second_text: Some(second.text),
            second_score: Some(second.score),
            second_reason: Some(second.reason),
        })
    }

    async fn call_scored_refine(
        &self,
        system_prompt: &str,
        user_content: &str,
        pass_name: &str,
    ) -> anyhow::Result<RefinePassResult> {
        let content = self.call_chat(system_prompt, user_content).await?;
        let json = extract_json_object(&content)?;
        let parsed: ScoredRefineResponse = serde_json::from_str(&json)?;
        let score = parse_score(parsed.score.as_ref());
        let text = parsed.text.trim().to_string();
        let reason = parsed.reason.trim().to_string();
        if text.is_empty() {
            anyhow::bail!("LLM {pass_name} pass returned empty text");
        };
        Ok(RefinePassResult {
            text,
            score,
            reason,
        })
    }

    async fn call_chat(&self, system_prompt: &str, user_content: &str) -> anyhow::Result<String> {
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
                    content: system_prompt,
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
            anyhow::bail!("LLM API failed with {status}: {raw}");
        }
        let parsed: ChatResponse = serde_json::from_str(&raw)?;
        let Some(choice) = parsed.choices.into_iter().next() else {
            anyhow::bail!("LLM API returned no choices");
        };
        Ok(choice.message.content.trim().to_string())
    }

    pub async fn test(&self) -> anyhow::Result<()> {
        let result = self.refine("测试 Python JSON code").await?;
        if result.is_empty() {
            anyhow::bail!("LLM test returned empty text");
        }
        Ok(())
    }
}

fn parse_score(value: Option<&serde_json::Value>) -> u8 {
    let score = match value {
        Some(serde_json::Value::Number(number)) => number.as_f64().unwrap_or(0.0),
        Some(serde_json::Value::String(text)) => text.trim().parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    };
    score.clamp(0.0, 100.0).round() as u8
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
        anyhow::bail!("LLM response contains no JSON object");
    };
    let Some(end) = trimmed.rfind('}') else {
        anyhow::bail!("LLM response contains no complete JSON object");
    };
    Ok(trimmed[start..=end].to_string())
}
