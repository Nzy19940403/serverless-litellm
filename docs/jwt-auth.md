# 公钥 / 私钥（RS256 JWT）鉴权

适合：**Cloud Run 允许公开访问** + 本地 agent 方便调用 + **私钥不进 Git / 不上云**。

## 架构

```text
GitHub 构建镜像（无密钥）
Cloud Run 环境变量：
  JWT_PUBLIC_KEY = 公钥 PEM     ← 只能验签，不能伪造 token
你的电脑（永不提交）：
  keys/private.pem              ← 签发 JWT
本地 agent / OpenAI SDK：
  api_key = mint 出来的 JWT
  base_url = https://xxx.run.app/v1
```

也支持传统 **`LITELLM_MASTER_KEY`**（对称共享密钥），可与 JWT **同时开启**（任一通过即可）。

## 1. 本机生成密钥对

```bash
cd serverless-litellm
bash scripts/gen_jwt_keys.sh ./keys
# → keys/private.pem  keys/public.pem
```

`*.pem` 已在 `.gitignore`。

## 2. Cloud Run 只配公钥

控制台 → 服务 → 变量与密钥：

| 变量 | 值 |
|------|-----|
| `JWT_PUBLIC_KEY` | `public.pem` 全文（换行可用 `\n` 写成一行） |
| `GCP_PROJECT` | 你的项目 |
| `GCP_LOCATION` | `global` |
| `LITELLM_MASTER_KEY` | （可选）备用长随机串，给 `/ui` 或紧急用 |

**不要**上传 `private.pem`。

可选：

- `JWT_ISSUER` / `JWT_AUDIENCE`：若设置，JWT 里必须带相同 `iss` / `aud`

## 3. 签发 Token（本机）

```bash
pip install PyJWT cryptography
python scripts/mint_jwt.py --key keys/private.pem --days 30 --sub my-cursor-agent
```

输出一长串 `eyJ...`，复制给 agent。

## 4. Agent 调用示例

```python
from openai import OpenAI

client = OpenAI(
    base_url="https://serverless-litellm-git-333500026338.europe-west1.run.app/v1",
    api_key="eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9...",  # mint 的 JWT
)
print(client.chat.completions.create(
    model="gemini-3.5-flash",
    messages=[{"role": "user", "content": "hi"}],
))
```

curl：

```bash
curl -sS "$BASE/v1/chat/completions" \
  -H "Authorization: Bearer $JWT" \
  -H "Content-Type: application/json" \
  -d '{"model":"gemini-3.5-flash","messages":[{"role":"user","content":"hi"}]}'
```

## 5. 安全说明

| 物品 | 放哪 |
|------|------|
| 代码 / Dockerfile | GitHub |
| 公钥 | Cloud Run 环境变量 / Secret Manager |
| 私钥 | 仅你的电脑 / 密码管理器 |
| JWT | 可过期；泄露后等过期或轮换密钥对 |

轮换：重新 `gen_jwt_keys` → 更新云上公钥 → 重新 mint → 旧私钥签的 token 立即失效。

## 6. Cloud Run 访问模式建议

- **允许公开访问** + JWT/Master Key：agent 最好用  
- **需要 IAM**：还要 Google identity token，和 OpenAI SDK 叠在一起很痛，不推荐给 agent 用  
