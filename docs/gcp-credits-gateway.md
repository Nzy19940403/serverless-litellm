# 用 Cloud Run 网关白嫖 / 消耗 GCP 赠金（Vertex Gemini）

目标链路：

```
你的 App
  → Cloud Run serverless-litellm  (Bearer = LITELLM_MASTER_KEY)
    → Vertex AI generateContent   (Bearer = 运行服务账号 token)
      → Gemini 3.6 Flash 等
        → 费用进 GCP 账单 → 试用赠金抵扣
```

**不要**用 `GEMINI_API_KEY`（AI Studio）走主路径——那条不一定扣赠金。  
主路径 model：`gemini-3.6-flash` / `gemini-flash` / `default`（`provider: vertex_gemini`）。

## 1. 控制台一次性准备

1. 项目已开 **试用 / 结算账号**（你的 project）
2. 启用 API：  
   [Vertex AI API](https://console.cloud.google.com/apis/library/aiplatform.googleapis.com) → **启用**
3. **IAM** → Cloud Run **运行服务账号**（常见 `数字-compute@developer.gserviceaccount.com`）  
   角色：**Vertex AI 用户** (`roles/aiplatform.user`)
4. （可选）Agent Platform / Model Garden 打开 Gemini 卡片确认模型 ID  
   若 3.6 不可用，可先改用 `gemini-2.5-flash`（config 里已有）

## 2. Cloud Run 环境变量

| 变量 | 值 |
|------|-----|
| `LITELLM_MASTER_KEY` | 自定网关密钥，如 `sk-my-gateway` |
| `GCP_PROJECT` | 项目 ID，如 `project-8d01f8fd-0b09-42c6-974` |
| `GCP_LOCATION` | 建议 `global`（与 Vertex 全局端点一致；404 再改 `us-central1`） |

**不需要** `GEMINI_API_KEY`（Vertex 主路径）。

部署新修订版本（push 含 `vertex_gemini` 的代码后等构建成功）。

## 3. 调用示例

```bash
export BASE="https://你的服务-xxxxx.run.app"
export KEY="sk-my-gateway"

curl -sS "$BASE/v1/chat/completions" \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-3.6-flash",
    "messages": [{"role":"user","content":"你好，介绍一下你自己"}]
  }'
```

Python：

```python
from openai import OpenAI
client = OpenAI(base_url=f"{BASE}/v1", api_key=KEY)
print(client.chat.completions.create(
    model="gemini-3.6-flash",
    messages=[{"role":"user","content":"hi"}],
).choices[0].message.content)
```

## 4. 确认在花赠金

1. 多调几次网关  
2. [账单 → 报告](https://console.cloud.google.com/billing)  
3. 应出现 **Vertex AI** 相关费用，且 **Credits** 下降  

只有 Cloud Run 小费用、没有 Vertex → 请求没打到 Vertex（查日志/模型名/权限）。

## 5. 本机调试（可选）

```bash
export LITELLM_MASTER_KEY=sk-test
export GCP_PROJECT=你的项目ID
export GCP_LOCATION=global
export VERTEX_ACCESS_TOKEN="$(gcloud auth print-access-token)"
cargo run --release
```

## 6. 模型名对照

| 客户端 model | Vertex 上游 |
|--------------|-------------|
| `gemini-3.6-flash` / `gemini-flash` / `default` | `gemini-3.6-flash` |
| `gemini-3.5-flash` | `gemini-3.5-flash` |
| `gemini-2.5-flash` | `gemini-2.5-flash`（兼容性好） |
| `gemini-3.1-flash-lite` | `gemini-3.1-flash-lite-preview` |
| `gemini-3.6-flash-studio` | AI Studio Key 路径（非赠金主路径） |

若某 ID 404，在 Model Garden 复制准确 Model ID 改 `config.yaml` 后重新部署。

## 7. 预算告警（强烈建议）

账单 → 预算与警报 → 设 $50/$100，避免试用变付费后超支。
