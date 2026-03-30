#!/usr/bin/env node
/**
 * Claude Code Proxy — wraps `claude-code --print` as an OpenAI-compatible HTTP server.
 * This lets the Rust eval harness call Claude through your OAuth credentials.
 *
 * Exposes: POST /v1/chat/completions (OpenAI format)
 * Routes to: npx @anthropic-ai/claude-code --print
 *
 * Usage: node scripts/claude_proxy.mjs
 * Then:  OPENAI_API_KEY=dummy OPENAI_BASE_URL=http://localhost:3033/v1 ./target/release/hsm-eval
 */

import http from 'http';
import { execSync } from 'child_process';

const PORT = 3033;
const MODEL = process.env.CLAUDE_MODEL || 'claude-sonnet-4-20250514';
let requestCount = 0;
let totalInputChars = 0;
let totalOutputChars = 0;

const server = http.createServer(async (req, res) => {
  if (req.method === 'GET' && req.url === '/health') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ status: 'ok', requests: requestCount, model: MODEL }));
    return;
  }

  if (req.method !== 'POST' || !req.url.includes('/chat/completions')) {
    res.writeHead(404);
    res.end('Not found');
    return;
  }

  let body = '';
  for await (const chunk of req) body += chunk;

  try {
    const parsed = JSON.parse(body);
    const messages = parsed.messages || [];

    // Flatten messages into a single prompt for --print mode
    let prompt = '';
    for (const msg of messages) {
      if (msg.role === 'system') {
        prompt += `[System: ${msg.content}]\n\n`;
      } else if (msg.role === 'user') {
        prompt += `User: ${msg.content}\n\n`;
      } else if (msg.role === 'assistant') {
        prompt += `Assistant: ${msg.content}\n\n`;
      }
    }

    const inputChars = prompt.length;
    totalInputChars += inputChars;
    requestCount++;

    const startMs = Date.now();

    // Call claude-code --print with the flattened prompt
    // Use a temp file to avoid shell escaping issues
    const fs = await import('fs');
    const tmpFile = `/tmp/claude_eval_${requestCount}.txt`;
    fs.writeFileSync(tmpFile, prompt);

    let output;
    try {
      output = execSync(
        `cat "${tmpFile}" | npx -y @anthropic-ai/claude-code --print --model ${MODEL} 2>/dev/null`,
        {
          encoding: 'utf8',
          timeout: 120000,
          maxBuffer: 1024 * 1024,
        }
      ).trim();
    } finally {
      try { fs.unlinkSync(tmpFile); } catch {}
    }

    const latencyMs = Date.now() - startMs;
    const outputChars = output.length;
    totalOutputChars += outputChars;

    // Estimate tokens (~4 chars per token)
    const promptTokens = Math.ceil(inputChars / 4);
    const completionTokens = Math.ceil(outputChars / 4);

    console.log(`[${requestCount}] ${latencyMs}ms | in=${promptTokens}tok out=${completionTokens}tok | ${output.substring(0, 60)}...`);

    // Return OpenAI-compatible response
    const response = {
      id: `chatcmpl-${requestCount}`,
      object: 'chat.completion',
      created: Math.floor(Date.now() / 1000),
      model: MODEL,
      choices: [{
        index: 0,
        message: { role: 'assistant', content: output },
        finish_reason: 'stop',
      }],
      usage: {
        prompt_tokens: promptTokens,
        completion_tokens: completionTokens,
        total_tokens: promptTokens + completionTokens,
      },
    };

    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(response));

  } catch (err) {
    console.error(`[${requestCount}] ERROR:`, err.message);
    res.writeHead(500, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({
      error: { message: err.message, type: 'server_error' }
    }));
  }
});

server.listen(PORT, () => {
  console.log(`\n🔌 Claude Code Proxy running on http://localhost:${PORT}`);
  console.log(`   Model: ${MODEL}`);
  console.log(`   Endpoint: POST /v1/chat/completions (OpenAI-compatible)`);
  console.log(`\n   Run eval with:`);
  console.log(`   OPENAI_API_KEY=dummy OPENAI_BASE_URL=http://localhost:${PORT}/v1 ./target/release/hsm-eval\n`);
});
