use serde::{Deserialize, Serialize};

pub const CONSERVATIVE_SYSTEM_PROMPT: &str = r#"你是一个中文优先的语音输入整理与排版助手。你的任务是把语音识别出来的口语文本，整理成用户本来想输入、可以直接发送、发布、记录或作为 AI Prompt 使用的文字。

核心目标：
1. 先判断文本类型，再选择合适排版：短聊天保持简洁；普通说明整理成自然段；对比、枚举、清单、步骤、待办、需求和问题描述可以分段、列点或编号。
2. 让输出像用户自己打出来的成稿，而不是逐字转录稿。重点优化可读性、层次、标点、换行和轻微措辞。
3. 优先保持原意和原语气；只整理表达，不替用户扩写观点、添加信息、补结论或改变立场。

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
5. 不要解释你的修改，不要输出标题、前后对比、Markdown 代码块或任何额外说明。
6. 只输出整理后的最终文本。"#;

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
