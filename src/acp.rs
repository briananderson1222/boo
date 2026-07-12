//! Generic runner for agents that speak the [Agent Client Protocol](https://agentclientprotocol.com)
//! (ACP) — newline-delimited JSON-RPC over the agent subprocess's stdio.
//!
//! One implementation drives any ACP agent (opencode `opencode acp`, kiro
//! `kiro-cli acp`, and others). The launch command is `config.acp_command`.
//!
//! For boo's non-interactive scheduled model we run a single prompt turn:
//! `initialize` → `session/new` → `session/prompt`, accumulate the agent's
//! `agent_message_chunk` text as the response, auto-answer tool-permission
//! requests per the job's trust settings, and finish when `session/prompt`
//! returns a `stopReason`.

use crate::config::Config;
use crate::error::{BooError, Result};
use crate::executor::ExecutionResult;
use crate::job::Job;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// Build the JSON-RPC response to a `session/request_permission` request,
/// based on the job's trust settings.
///
/// - `trust_all_tools` → approve every tool.
/// - `trust_tools` → approve when the requested tool's name matches an entry.
/// - neither → deny (a scheduled job has no human to prompt).
pub fn permission_response(job: &Job, id: &Value, params: &Value) -> Value {
    let options = params["options"].as_array();
    let option_of = |kinds: &[&str]| -> Option<String> {
        options.and_then(|opts| {
            opts.iter()
                .find(|o| kinds.contains(&o["kind"].as_str().unwrap_or("")))
                .and_then(|o| o["optionId"].as_str().map(String::from))
        })
    };
    let approve = || {
        option_of(&["allow_once", "allow_always"]).or_else(|| {
            options
                .and_then(|o| o.first())
                .and_then(|o| o["optionId"].as_str().map(String::from))
        })
    };
    let deny = || json!({"jsonrpc":"2.0","id":id,"result":{"outcome":{"outcome":"cancelled"}}});

    let allowed = if job.trust_all_tools {
        true
    } else if let Some(ref tools) = job.trust_tools {
        let name = params["toolCall"]["title"]
            .as_str()
            .or_else(|| params["toolCall"]["kind"].as_str())
            .or_else(|| params["toolCall"]["toolCallId"].as_str())
            .unwrap_or("");
        tools
            .split([',', ' '])
            .filter(|t| !t.is_empty())
            .any(|t| name.contains(t))
    } else {
        false
    };

    if allowed {
        match approve() {
            Some(opt) => {
                json!({"jsonrpc":"2.0","id":id,"result":{"outcome":{"outcome":"selected","optionId":opt}}})
            }
            None => deny(),
        }
    } else {
        deny()
    }
}

/// Run a single ACP prompt turn and return its result.
pub async fn run_acp(
    job: &Job,
    config: &Config,
    log_path: &Path,
    on_spawn: Option<&(dyn Fn(u32) + Send + Sync)>,
) -> Result<ExecutionResult> {
    let command = config
        .acp_command
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            BooError::Other(
                "the `acp` runner needs `acp_command` set in config, e.g. \"opencode acp\" or \"kiro-cli acp\""
                    .into(),
            )
        })?;
    let mut parts = command.split_whitespace();
    let program = parts.next().unwrap(); // command is non-empty
    let args: Vec<&str> = parts.collect();

    let start = Instant::now();
    let timeout_secs = job.timeout_secs.unwrap_or(config.default_timeout_secs);

    let mut child = Command::new(program)
        .args(&args)
        .current_dir(&job.working_dir)
        .env("BOO_NON_INTERACTIVE", "1")
        .env("BOO_JOB_NAME", &job.name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(BooError::Io)?;

    if let (Some(cb), Some(pid)) = (on_spawn, child.id()) {
        cb(pid);
    }

    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();

    let prompt = job.prompt.clone();
    let cwd = job.working_dir.to_string_lossy().to_string();

    let turn = tokio::time::timeout(Duration::from_secs(timeout_secs), async {
        async fn send(stdin: &mut tokio::process::ChildStdin, msg: &Value) -> Result<()> {
            let mut line = serde_json::to_string(msg)?;
            line.push('\n');
            stdin.write_all(line.as_bytes()).await.map_err(BooError::Io)
        }

        send(
            &mut stdin,
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":1,
                "clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false}}
            }}),
        )
        .await?;

        let mut response = String::new();
        let mut transcript = String::new();

        while let Some(line) = lines.next_line().await.map_err(BooError::Io)? {
            if line.trim().is_empty() {
                continue;
            }
            transcript.push_str(&line);
            transcript.push('\n');
            let msg: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if msg["id"] == json!(1) && !msg["result"].is_null() {
                send(
                    &mut stdin,
                    &json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{
                        "cwd": cwd, "mcpServers": []
                    }}),
                )
                .await?;
            } else if msg["id"] == json!(2) && !msg["result"].is_null() {
                let session_id = msg["result"]["sessionId"].as_str().unwrap_or_default();
                send(
                    &mut stdin,
                    &json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{
                        "sessionId": session_id,
                        "prompt": [{"type":"text","text": prompt}]
                    }}),
                )
                .await?;
            } else if msg["id"] == json!(3) && !msg["result"].is_null() {
                let stop = msg["result"]["stopReason"].as_str().unwrap_or("");
                return Ok((response, transcript, stop == "end_turn"));
            } else if msg["method"] == json!("session/update") {
                let update = &msg["params"]["update"];
                if update["sessionUpdate"] == json!("agent_message_chunk") {
                    if let Some(t) = update["content"]["text"].as_str() {
                        response.push_str(t);
                    }
                }
            } else if msg["method"] == json!("session/request_permission") {
                let reply = permission_response(job, &msg["id"], &msg["params"]);
                send(&mut stdin, &reply).await?;
            } else if !msg["error"].is_null() && !msg["id"].is_null() {
                return Err(BooError::Other(format!(
                    "ACP agent returned an error: {}",
                    msg["error"]
                )));
            }
        }

        Err(BooError::Other(
            "ACP agent closed the connection before the prompt turn completed".into(),
        ))
    })
    .await;

    let duration_secs = start.elapsed().as_secs_f64();

    match turn {
        Ok(Ok((response, transcript, success))) => {
            let _ = tokio::fs::write(log_path, &transcript).await;
            let response_path = log_path.with_extension("response");
            let _ = tokio::fs::write(&response_path, &response).await;
            crate::config::restrict_file_permissions(log_path);
            crate::config::restrict_file_permissions(&response_path);
            Ok(ExecutionResult {
                exit_code: Some(if success { 0 } else { 1 }),
                success,
                duration_secs,
                output_path: log_path.to_path_buf(),
                response: Some(response),
            })
        }
        Ok(Err(e)) => {
            let _ = child.kill().await;
            Err(e)
        }
        Err(_) => {
            if let Some(id) = child.id() {
                crate::kill_process_group(id, false);
            }
            let _ = child.kill().await;
            let _ = tokio::fs::write(
                log_path,
                format!("boo: ACP job timed out after {timeout_secs}s; process killed\n"),
            )
            .await;
            Err(BooError::JobTimeout(timeout_secs))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_with(trust_all: bool, trust_tools: Option<&str>) -> Job {
        let mut j = Job::new("t", "* * * * *", "hi", std::env::temp_dir());
        j.trust_all_tools = trust_all;
        j.trust_tools = trust_tools.map(String::from);
        j
    }

    fn perm_params() -> Value {
        json!({
            "toolCall": {"title": "read_file", "kind": "read"},
            "options": [
                {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                {"optionId": "reject", "name": "Reject", "kind": "reject_once"}
            ]
        })
    }

    #[test]
    fn trust_all_approves() {
        let r = permission_response(&job_with(true, None), &json!(5), &perm_params());
        assert_eq!(r["result"]["outcome"]["outcome"], "selected");
        assert_eq!(r["result"]["outcome"]["optionId"], "allow");
        assert_eq!(r["id"], 5);
    }

    #[test]
    fn no_trust_denies() {
        let r = permission_response(&job_with(false, None), &json!(5), &perm_params());
        assert_eq!(r["result"]["outcome"]["outcome"], "cancelled");
    }

    #[test]
    fn trust_tools_matches_by_name() {
        let approved = permission_response(
            &job_with(false, Some("read_file,write")),
            &json!(1),
            &perm_params(),
        );
        assert_eq!(approved["result"]["outcome"]["outcome"], "selected");
        let denied = permission_response(&job_with(false, Some("bash")), &json!(1), &perm_params());
        assert_eq!(denied["result"]["outcome"]["outcome"], "cancelled");
    }
}
