# CLI agents and direct model APIs

Spindle supports two distinct entry points: interactive installed coding-agent CLIs and direct single-turn model API requests. These are complementary workflows with different authentication and lifecycle behavior. Development is tracked in [issue #8](https://github.com/krishangnaikar/Spindle/issues/8).

## Installed agent CLIs

Start the Linux session host, then launch an installed Claude Code or Codex executable:

```sh
spindle --state-dir .state host
# In another terminal:
spindle --state-dir .state agent claude --cwd /absolute/project/path
spindle --state-dir .state agent codex --cwd /absolute/project/path --model MODEL
```

`agent` accepts `claude` or `codex`, optional `--id UUID`, `--cwd PATH`, and `--model MODEL`. It translates the selection into the existing structured PTY launch. The executable must already be installed and discoverable by the host. Authenticate through that CLI's own supported login flow before relying on unattended operation. See the official [Claude Code authentication](https://code.claude.com/docs/en/authentication) and [Codex authentication](https://learn.chatgpt.com/docs/auth) documentation (checked 2026-11-07).

Spindle does not extract or repurpose a CLI login token as an API key. The launched CLI controls its authentication, account access and billing mode. Availability of a model or subscription benefit depends on that CLI and account; creating a Spindle session does not establish entitlement. The existing lifecycle operations apply: use the returned session ID and generation for input, resize, output and cancellation.

This initial adapter launches the CLI. It does not yet attest model readiness, implement engine-native tool protocol mediation, recover a semantic conversation or satisfy the full upstream engine-parity requirements. PTY launch tests and command-construction fixtures are not live subscription/account validation.

## Direct model APIs

`ask` reads a prompt file and performs one request to OpenAI, Anthropic or OpenRouter:

```sh
# OPENROUTER_API_KEY must already be present in the invoking environment.
spindle ask --provider openrouter --model openrouter/free --prompt-file prompt.txt

# Direct paid-capable routing requires explicit opt-in.
spindle ask --provider openai --model MODEL --prompt-file prompt.txt --allow-paid
spindle ask --provider anthropic --model MODEL --prompt-file prompt.txt --allow-paid
```

The command also accepts `--max-output-tokens` (default 1024) and `--api-key-env NAME`. The latter names an environment variable; it is not a key value. Default names are `OPENAI_API_KEY`, `ANTHROPIC_API_KEY` and `OPENROUTER_API_KEY`. Provision the credential outside command arguments. The application reads the selected variable when the user invokes `ask`; no discovery by dumping the environment is needed.

Without `--allow-paid`, only OpenRouter's `openrouter/free` router or an explicitly `:free` model is accepted. The request additionally applies zero-price routing constraints and disables fallback. OpenRouter documents its [free router](https://openrouter.ai/docs/guides/routing/routers/free-router) and [provider selection controls](https://openrouter.ai/docs/guides/routing/provider-selection); Spindle applies the stricter local allowlist before sending a request. Direct OpenAI/Anthropic routes require `--allow-paid`. This flag authorizes a paid-capable request; it does not set a dollar budget, guarantee availability or prove that a provider will accept an account. Output-token limits are not total-cost limits.

Production endpoints are fixed. Requests have a deadline and response-size cap, do not follow redirects and do not automatically retry. An ambiguous network failure can occur after a provider accepts work, so retrying is a new user decision. A free route may be unavailable or rate-limited; it does not silently become a paid route.

The direct path is deliberately single-turn text generation: no tool execution, streaming or durable chat state. It does not use the interactive CLI's login, terminal or conversation. Prompt files and responses remain subject to the selected provider's handling; local-first session storage does not mean requests stay on the machine.

## Optional session lifecycle for an API request

A direct request can be run as an ordinary session child when PTY output replay and lifecycle tracking are useful:

```sh
spindle --state-dir .state launch /absolute/path/to/spindle ask \
  --provider openrouter --model openrouter/free --prompt-file /absolute/path/prompt.txt
```

The host must have the named credential in its own environment, since children inherit the host environment. The invoking shell's newer environment does not update an already-running host. The executable and prompt paths are resolved on the host machine.

The prompt-file path and other launch arguments are persisted in local session metadata; the response appears in terminal output and bounded memory replay. Keep the state directory and prompt file private. Never put an API key or sensitive prompt text in launch arguments. This optional wrapper does not create durable provider-side conversation state, restore a completed API response after host crash, or make a request safe to retry.

## Validation boundary

Offline fixtures can verify request encoding, provider-specific headers, free-route enforcement, failure handling, size limits, timeout and redirect behavior. They cannot validate actual credentials, account permissions, subscription eligibility, current model availability or real provider billing. No live model/account request is claimed by this milestone's tests. Native CLI login and per-engine readiness still require explicit real-account validation.
