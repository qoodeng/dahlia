#!/bin/sh
# Starts a vendor's own coding CLI as an ACP agent for Fabro's implement and fix steps.
# Each CLI signs in with its own account; Fabro never sees those credentials.
#   grok    Grok Build over stdio, approving its own tool calls
#   cursor  Cursor's CLI in ACP mode, allowed to run commands (--force)
#   codex   OpenAI's Codex CLI through the ACP project's codex-acp, on the ChatGPT login
#   claude  Anthropic's Agent SDK through the ACP project's claude-agent-acp, on the claude.ai login
# Codex and Claude use their subscriptions only while no API key is set, so those are cleared.
# Usage: acp-agent.sh <agent> [model]. Fabro runs acp.command without a shell, so the
# model is an argument rather than an environment prefix.
set -eu

agent="${1:-}"
model="${2:-}"
case "$agent" in
grok)
    exec /opt/homebrew/bin/agent agent --always-approve -m "${model:-grok-4.7}" stdio
    ;;
cursor)
    exec "$HOME/.local/bin/cursor-agent" --force --model "${model:-composer-2.5}" acp
    ;;
codex)
    unset OPENAI_API_KEY CODEX_API_KEY
    # Full access because builds need Xcode, rustup and ~/.cargo outside the workspace.
    config='"approval_policy":"never","sandbox_mode":"danger-full-access"'
    if [ -n "$model" ]; then
        config="$config,\"model\":\"$model\""
    fi
    CODEX_CONFIG="{$config}" exec codex-acp
    ;;
claude)
    unset ANTHROPIC_API_KEY ANTHROPIC_AUTH_TOKEN
    if [ -n "$model" ]; then
        export ANTHROPIC_MODEL="$model"
    fi
    exec claude-agent-acp
    ;;
*)
    echo "acp-agent.sh: unknown agent '$agent' (expected grok, cursor, codex or claude)" >&2
    exit 2
    ;;
esac
