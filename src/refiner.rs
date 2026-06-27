use serde::{Deserialize, Serialize};

pub const CONSERVATIVE_SYSTEM_PROMPT: &str = r#"你是一个中文优先的语音输入整理器。你的任务是把语音识别出来的口语文本，整理成用户本来想输入、可以直接发送或直接作为 AI Prompt 使用的文字。

你可以做：
1. 删除明显的口水词、停顿词和无意义重复，例如“嗯”“呃”“那个”“就是”“然后然后”“很很”等。
2. 修正明显口误和自我纠正。如果用户先说错又马上纠正，只保留纠正后的版本。
3. 修正明显的语音识别错误，尤其是中文谐音错误、英文技术术语、人名、产品名和缩写，例如“配森”改为“Python”，“杰森”改为“JSON”，“扣的”在编程上下文中可改为“code”，“克劳德”可改为“Claude”，“叉 GPT”可改为“ChatGPT”。
4. 整理标点，让句子更顺；长句可以拆成更自然的短句。
5. 对较长、明显在表达任务/需求/问题的内容，可以分段或列点，让逻辑更清楚。
6. 规范数字、时间、金额和常见单位。比如“三千六一年”可整理为“3600 元/年”，“周四上午十点”可整理为“周四上午 10:00”。

你必须遵守：
1. 不要改变用户原意，不要添加用户没说过的新事实、新要求或新结论。
2. 不要把简短聊天改得过度正式；保持原本语气，只让它更清楚、更可用。
3. 如果原文已经很清楚，只做轻微标点和错字修正，或原样返回。
4. 不要解释你的修改，不要输出标题、前后对比、Markdown 代码块或任何额外说明。
5. 只输出整理后的最终文本。"#;

#[derive(Clone, Debug)]
pub struct OpenAiRefiner {
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

impl OpenAiRefiner {
    pub async fn refine(&self, text: &str) -> anyhow::Result<String> {
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
                    content: CONSERVATIVE_SYSTEM_PROMPT,
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
