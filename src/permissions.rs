use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use github_copilot_sdk::handler::{PermissionHandler, PermissionResult};
use github_copilot_sdk::types::{
    PermissionRequestData, PermissionRequestKind, RequestId, SessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalCategory {
    Shell,
    Task,
    ExternalWrite,
}

impl ApprovalCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Task => "task",
            Self::ExternalWrite => "external write",
        }
    }

    pub fn supports_trust(self) -> bool {
        matches!(self, Self::Shell | Self::Task)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    ApproveOnce,
    Deny,
    Trust,
}

#[derive(Debug)]
pub struct ApprovalRequest {
    pub category: ApprovalCategory,
    pub tool_name: String,
    pub details: String,
    pub respond_to: oneshot::Sender<ApprovalDecision>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct PersistedTrust {
    shell: bool,
    task: bool,
}

struct TrustStore {
    directory: PathBuf,
    sessions: Mutex<HashMap<String, PersistedTrust>>,
}

impl TrustStore {
    fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn is_trusted(&self, session_id: &SessionId, category: ApprovalCategory) -> bool {
        let mut sessions = self
            .sessions
            .lock()
            .expect("permission trust lock poisoned");
        let trust = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| self.load(session_id));
        match category {
            ApprovalCategory::Shell => trust.shell,
            ApprovalCategory::Task => trust.task,
            ApprovalCategory::ExternalWrite => false,
        }
    }

    fn trust(&self, session_id: &SessionId, category: ApprovalCategory) -> std::io::Result<()> {
        let mut sessions = self
            .sessions
            .lock()
            .expect("permission trust lock poisoned");
        let trust = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| self.load(session_id));
        match category {
            ApprovalCategory::Shell => trust.shell = true,
            ApprovalCategory::Task => trust.task = true,
            ApprovalCategory::ExternalWrite => return Ok(()),
        }
        fs::create_dir_all(&self.directory)?;
        let contents = serde_json::to_vec_pretty(trust).expect("trust state is serializable");
        fs::write(self.path_for(session_id), contents)
    }

    fn load(&self, session_id: &SessionId) -> PersistedTrust {
        fs::read(self.path_for(session_id))
            .ok()
            .and_then(|contents| serde_json::from_slice(&contents).ok())
            .unwrap_or_default()
    }

    fn path_for(&self, session_id: &SessionId) -> PathBuf {
        self.directory
            .join(format!("{}.json", safe_session_filename(session_id)))
    }
}

pub fn permission_handler(
    workspace_root: PathBuf,
) -> (
    Arc<dyn PermissionHandler>,
    mpsc::UnboundedReceiver<ApprovalRequest>,
) {
    let (requests, receiver) = mpsc::unbounded_channel();
    let handler = PermissionGate {
        workspace_root,
        requests,
        trust_store: TrustStore::new(default_trust_directory()),
    };
    (Arc::new(handler), receiver)
}

struct PermissionGate {
    workspace_root: PathBuf,
    requests: mpsc::UnboundedSender<ApprovalRequest>,
    trust_store: TrustStore,
}

#[async_trait]
impl PermissionHandler for PermissionGate {
    async fn handle(
        &self,
        session_id: SessionId,
        _request_id: RequestId,
        data: PermissionRequestData,
    ) -> PermissionResult {
        match classify_request(&data, &self.workspace_root) {
            PolicyDecision::Approve => PermissionResult::approve_once(),
            PolicyDecision::Deny(feedback) => PermissionResult::reject(Some(feedback)),
            PolicyDecision::Confirm {
                category,
                tool_name,
                details,
            } => {
                if category.supports_trust() && self.trust_store.is_trusted(&session_id, category) {
                    return PermissionResult::approve_once();
                }

                let (respond_to, response) = oneshot::channel();
                let request = ApprovalRequest {
                    category,
                    tool_name,
                    details,
                    respond_to,
                };
                if self.requests.send(request).is_err() {
                    return PermissionResult::reject(Some(
                        "picopilot is not available to confirm this operation".to_string(),
                    ));
                }

                match response.await {
                    Ok(ApprovalDecision::ApproveOnce) => PermissionResult::approve_once(),
                    Ok(ApprovalDecision::Deny) => {
                        PermissionResult::reject(Some("denied by user".to_string()))
                    }
                    Ok(ApprovalDecision::Trust) => {
                        if !category.supports_trust() {
                            PermissionResult::reject(Some(
                                "external writes cannot be trusted for the session".to_string(),
                            ))
                        } else if self.trust_store.trust(&session_id, category).is_err() {
                            PermissionResult::reject(Some(
                                "could not persist session trust; operation denied".to_string(),
                            ))
                        } else {
                            PermissionResult::approve_once()
                        }
                    }
                    Err(_) => PermissionResult::reject(Some(
                        "picopilot stopped before confirming this operation".to_string(),
                    )),
                }
            }
        }
    }
}

enum PolicyDecision {
    Approve,
    Deny(String),
    Confirm {
        category: ApprovalCategory,
        tool_name: String,
        details: String,
    },
}

fn classify_request(data: &PermissionRequestData, workspace_root: &Path) -> PolicyDecision {
    let tool_name = first_string(
        &data.extra,
        &["toolName", "tool_name", "tool", "name", "identifier"],
    )
    .unwrap_or_else(|| match data.kind {
        Some(PermissionRequestKind::Shell) => "shell".to_string(),
        _ => "unknown tool".to_string(),
    });
    if is_task_tool(&tool_name) {
        return PolicyDecision::Confirm {
            category: ApprovalCategory::Task,
            details: request_details(&data.extra, ApprovalCategory::Task),
            tool_name,
        };
    }

    match data.kind {
        Some(PermissionRequestKind::Read) => PolicyDecision::Approve,
        Some(PermissionRequestKind::Write) => {
            if write_paths_are_safe(&data.extra, workspace_root) {
                PolicyDecision::Approve
            } else {
                PolicyDecision::Confirm {
                    category: ApprovalCategory::ExternalWrite,
                    details: request_details(&data.extra, ApprovalCategory::ExternalWrite),
                    tool_name,
                }
            }
        }
        Some(PermissionRequestKind::Shell) => PolicyDecision::Confirm {
            category: ApprovalCategory::Shell,
            details: request_details(&data.extra, ApprovalCategory::Shell),
            tool_name,
        },
        Some(PermissionRequestKind::Unknown) | None => {
            match inferred_policy(&tool_name, &data.extra, workspace_root) {
                InferredPolicy::Approve => PolicyDecision::Approve,
                InferredPolicy::Deny => PolicyDecision::Deny(
                    "picopilot does not permit this permission category".to_string(),
                ),
                InferredPolicy::Confirm(ApprovalCategory::Shell) => PolicyDecision::Confirm {
                    category: ApprovalCategory::Shell,
                    details: request_details(&data.extra, ApprovalCategory::Shell),
                    tool_name,
                },
                InferredPolicy::Confirm(ApprovalCategory::Task) => PolicyDecision::Confirm {
                    category: ApprovalCategory::Task,
                    details: request_details(&data.extra, ApprovalCategory::Task),
                    tool_name,
                },
                InferredPolicy::Confirm(ApprovalCategory::ExternalWrite) => {
                    PolicyDecision::Confirm {
                        category: ApprovalCategory::ExternalWrite,
                        details: request_details(&data.extra, ApprovalCategory::ExternalWrite),
                        tool_name,
                    }
                }
            }
        }
        Some(PermissionRequestKind::Url)
        | Some(PermissionRequestKind::Mcp)
        | Some(PermissionRequestKind::CustomTool)
        | Some(PermissionRequestKind::Memory)
        | Some(PermissionRequestKind::Hook) => {
            PolicyDecision::Deny("picopilot does not permit this permission category".to_string())
        }
        _ => PolicyDecision::Deny("picopilot does not permit this permission category".to_string()),
    }
}

enum InferredPolicy {
    Approve,
    Deny,
    Confirm(ApprovalCategory),
}

fn inferred_policy(tool_name: &str, extra: &Value, workspace_root: &Path) -> InferredPolicy {
    if is_task_tool(tool_name) {
        return InferredPolicy::Confirm(ApprovalCategory::Task);
    }
    let lower = tool_name.to_ascii_lowercase();
    if matches!(lower.as_str(), "view" | "read" | "grep" | "glob") {
        return InferredPolicy::Approve;
    }
    if matches!(lower.as_str(), "edit" | "create") {
        return if write_paths_are_safe(extra, workspace_root) {
            InferredPolicy::Approve
        } else {
            InferredPolicy::Confirm(ApprovalCategory::ExternalWrite)
        };
    }
    if lower.contains("shell") || lower.contains("bash") || lower.contains("powershell") {
        return InferredPolicy::Confirm(ApprovalCategory::Shell);
    }
    if first_string(extra, &["command", "cmd", "script"]).is_some() {
        return InferredPolicy::Confirm(ApprovalCategory::Shell);
    }
    InferredPolicy::Deny
}

fn is_task_tool(tool_name: &str) -> bool {
    matches!(
        tool_name.to_ascii_lowercase().as_str(),
        "task" | "fleet.start" | "task.start" | "subagent" | "sub-agent"
    )
}

fn request_details(extra: &Value, category: ApprovalCategory) -> String {
    let keys = match category {
        ApprovalCategory::Shell => ["fullCommandText", "command", "cmd", "script"],
        ApprovalCategory::Task => ["prompt", "description", "task", "agentName"],
        ApprovalCategory::ExternalWrite => ["path", "filePath", "file_path", "description"],
    };
    first_string(extra, &keys).unwrap_or_else(|| "details unavailable".to_string())
}

fn write_paths_are_safe(extra: &Value, workspace_root: &Path) -> bool {
    let mut paths = Vec::new();
    collect_keyed_strings(
        extra,
        &[
            "path",
            "filePath",
            "file_path",
            "possiblePaths",
            "possible_paths",
        ],
        &mut paths,
    );
    !paths.is_empty()
        && paths
            .iter()
            .all(|path| is_within_workspace(workspace_root, Path::new(path)))
}

fn is_within_workspace(workspace_root: &Path, path: &Path) -> bool {
    let root = if workspace_root.is_absolute() {
        workspace_root.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(workspace_root)
    };
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let Ok(root) = canonicalize_with_missing_tail(&root) else {
        return false;
    };
    let Ok(candidate) = canonicalize_with_missing_tail(&candidate) else {
        return false;
    };
    let root_key = path_key(&root);
    let candidate_key = path_key(&candidate);
    candidate_key == root_key || candidate_key.starts_with(&(root_key + "/"))
}

fn canonicalize_with_missing_tail(path: &Path) -> std::io::Result<PathBuf> {
    let mut ancestor = path.to_path_buf();
    let mut missing = Vec::new();
    while !ancestor.exists() {
        let Some(name) = ancestor.file_name() else {
            return fs::canonicalize(path);
        };
        missing.push(name.to_os_string());
        if !ancestor.pop() {
            return fs::canonicalize(path);
        }
    }

    let mut resolved = fs::canonicalize(ancestor)?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn path_key(path: &Path) -> String {
    let key = path
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    let mut values = Vec::new();
    collect_keyed_strings(value, keys, &mut values);
    values.into_iter().next()
}

fn collect_keyed_strings(value: &Value, keys: &[&str], values: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if keys
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(key))
                {
                    collect_scalar_strings(child, values);
                }
                collect_keyed_strings(child, keys, values);
            }
        }
        Value::Array(array) => {
            for child in array {
                collect_keyed_strings(child, keys, values);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn collect_scalar_strings(value: &Value, values: &mut Vec<String>) {
    match value {
        Value::String(value) => values.push(value.clone()),
        Value::Array(array) => {
            for child in array {
                collect_scalar_strings(child, values);
            }
        }
        _ => {}
    }
}

fn safe_session_filename(session_id: &SessionId) -> String {
    session_id
        .to_string()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn default_trust_directory() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".picopilot").join("sessions")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use github_copilot_sdk::handler::{PermissionHandler, PermissionResult};
    use github_copilot_sdk::types::{
        PermissionRequestData, PermissionRequestKind, RequestId, SessionId,
    };
    use serde_json::json;
    use tokio::sync::mpsc;

    use super::{
        permission_handler, ApprovalCategory, ApprovalDecision, PermissionGate, TrustStore,
    };

    #[tokio::test]
    async fn auto_approves_read_requests_without_waiting_for_the_ui() {
        let (handler, mut requests) = permission_handler(std::env::current_dir().unwrap());
        let request = PermissionRequestData {
            kind: Some(PermissionRequestKind::Read),
            extra: json!({ "path": "src/lib.rs" }),
            ..Default::default()
        };

        let result = handler
            .handle(
                SessionId::from("session-1"),
                RequestId::new("request-1"),
                request,
            )
            .await;

        assert!(matches!(result, PermissionResult::Decision { .. }));
        assert!(requests.try_recv().is_err());
    }

    #[tokio::test]
    async fn auto_approves_writes_inside_the_workspace() {
        let workspace = std::env::current_dir().unwrap();
        let (handler, mut requests) = permission_handler(workspace.clone());
        let request = PermissionRequestData {
            kind: Some(PermissionRequestKind::Write),
            extra: json!({
                "path": workspace.join("src").join("lib.rs")
            }),
            ..Default::default()
        };

        let result = handler
            .handle(
                SessionId::from("session-write"),
                RequestId::new("request-write"),
                request,
            )
            .await;

        assert!(matches!(result, PermissionResult::Decision { .. }));
        assert!(requests.try_recv().is_err());
    }

    #[tokio::test]
    async fn confirms_writes_outside_the_workspace_and_through_traversal() {
        let (handler, mut requests, trust_directory) = test_gate();
        for (index, path) in ["../outside.txt", "C:\\outside.txt"]
            .into_iter()
            .enumerate()
        {
            let task = tokio::spawn({
                let handler = handler.clone();
                async move {
                    handler
                        .handle(
                            SessionId::from("session-write"),
                            RequestId::new(format!("request-outside-{index}")),
                            PermissionRequestData {
                                kind: Some(PermissionRequestKind::Write),
                                extra: json!({ "path": path }),
                                ..Default::default()
                            },
                        )
                        .await
                }
            });
            let request = requests.recv().await.expect("outside write should ask");
            assert_eq!(request.category, ApprovalCategory::ExternalWrite);
            assert!(!request.category.supports_trust());
            request
                .respond_to
                .send(ApprovalDecision::ApproveOnce)
                .expect("permission handler should be waiting");
            let result = task.await.expect("permission task should finish");

            assert!(matches!(result, PermissionResult::Decision { .. }));
        }
        assert!(requests.try_recv().is_err());
        assert!(!trust_directory.exists());
        cleanup_trust_directory(trust_directory);
    }

    #[tokio::test]
    async fn rejects_unsupported_permission_categories() {
        let (handler, mut requests) = permission_handler(std::env::current_dir().unwrap());
        let result = handler
            .handle(
                SessionId::from("session-url"),
                RequestId::new("request-url"),
                PermissionRequestData {
                    kind: Some(PermissionRequestKind::Url),
                    extra: json!({ "url": "https://example.com" }),
                    ..Default::default()
                },
            )
            .await;

        assert!(matches!(result, PermissionResult::Decision { .. }));
        assert!(requests.try_recv().is_err());
    }

    #[tokio::test]
    async fn blocks_shell_and_task_requests_until_the_ui_decides() {
        let (handler, mut requests, trust_directory) = test_gate();

        let shell = confirm_request(
            handler.clone(),
            SessionId::from("session-confirm"),
            PermissionRequestData {
                kind: Some(PermissionRequestKind::Shell),
                extra: json!({ "command": "cargo test" }),
                ..Default::default()
            },
            &mut requests,
            ApprovalDecision::ApproveOnce,
        )
        .await;
        assert!(matches!(shell, PermissionResult::Decision { .. }));

        let task = confirm_request(
            handler,
            SessionId::from("session-confirm"),
            PermissionRequestData {
                kind: Some(PermissionRequestKind::Unknown),
                extra: json!({ "toolName": "task", "prompt": "inspect the code" }),
                ..Default::default()
            },
            &mut requests,
            ApprovalDecision::Deny,
        )
        .await;
        assert!(matches!(task, PermissionResult::Decision { .. }));
        assert!(!trust_directory.exists());
        cleanup_trust_directory(trust_directory);
    }

    #[tokio::test]
    async fn describes_realistic_nested_shell_requests() {
        let (handler, mut requests, trust_directory) = test_gate();
        let task = tokio::spawn(async move {
            handler
                .handle(
                    SessionId::from("session-powershell"),
                    RequestId::new("request-powershell"),
                    PermissionRequestData {
                        kind: Some(PermissionRequestKind::Shell),
                        extra: json!({
                            "permissionRequest": {
                                "kind": "shell",
                                "fullCommandText": "Get-CimInstance Win32_OperatingSystem",
                                "commands": [{
                                    "identifier": "Get-CimInstance",
                                    "readOnly": true
                                }]
                            }
                        }),
                        ..Default::default()
                    },
                )
                .await
        });

        let request = requests.recv().await.expect("shell request should ask");
        assert_eq!(request.tool_name, "Get-CimInstance");
        assert_eq!(request.details, "Get-CimInstance Win32_OperatingSystem");
        request
            .respond_to
            .send(ApprovalDecision::Deny)
            .expect("permission handler should be waiting");
        let _ = task.await.expect("permission task should finish");
        cleanup_trust_directory(trust_directory);
    }

    #[tokio::test]
    async fn persists_task_trust_without_trusting_shell() {
        let (handler, mut requests, trust_directory) = test_gate();
        let task = confirm_request(
            handler,
            SessionId::from("session-trust"),
            PermissionRequestData {
                kind: Some(PermissionRequestKind::Unknown),
                extra: json!({ "toolName": "fleet.start", "prompt": "parallel work" }),
                ..Default::default()
            },
            &mut requests,
            ApprovalDecision::Trust,
        )
        .await;
        assert!(matches!(task, PermissionResult::Decision { .. }));

        let (handler, mut requests) = test_gate_with_directory(trust_directory.clone());
        let trusted_task = handler
            .handle(
                SessionId::from("session-trust"),
                RequestId::new("request-trusted-task"),
                PermissionRequestData {
                    kind: Some(PermissionRequestKind::Unknown),
                    extra: json!({ "toolName": "task", "prompt": "another task" }),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(trusted_task, PermissionResult::Decision { .. }));
        assert!(requests.try_recv().is_err());

        let shell = confirm_request(
            handler,
            SessionId::from("session-trust"),
            PermissionRequestData {
                kind: Some(PermissionRequestKind::Shell),
                extra: json!({ "command": "cargo test" }),
                ..Default::default()
            },
            &mut requests,
            ApprovalDecision::Deny,
        )
        .await;
        assert!(matches!(shell, PermissionResult::Decision { .. }));
        cleanup_trust_directory(trust_directory);
    }

    async fn confirm_request(
        handler: Arc<PermissionGate>,
        session_id: SessionId,
        data: PermissionRequestData,
        requests: &mut mpsc::UnboundedReceiver<super::ApprovalRequest>,
        decision: ApprovalDecision,
    ) -> PermissionResult {
        let task = tokio::spawn(async move {
            handler
                .handle(session_id, RequestId::new("request-confirm"), data)
                .await
        });
        let request = requests
            .recv()
            .await
            .expect("confirmation should reach the UI queue");
        assert!(matches!(
            request.category,
            ApprovalCategory::Shell | ApprovalCategory::Task
        ));
        request
            .respond_to
            .send(decision)
            .expect("permission handler should still be waiting");
        task.await.expect("permission task should finish")
    }

    fn test_gate() -> (
        Arc<PermissionGate>,
        mpsc::UnboundedReceiver<super::ApprovalRequest>,
        PathBuf,
    ) {
        static NEXT_TEST_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "picopilot-permissions-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        cleanup_trust_directory(directory.clone());
        let (handler, requests) = test_gate_with_directory(directory.clone());
        (handler, requests, directory)
    }

    fn test_gate_with_directory(
        directory: PathBuf,
    ) -> (
        Arc<PermissionGate>,
        mpsc::UnboundedReceiver<super::ApprovalRequest>,
    ) {
        let (requests, receiver) = mpsc::unbounded_channel();
        let gate = PermissionGate {
            workspace_root: std::env::current_dir().unwrap(),
            requests,
            trust_store: TrustStore::new(directory),
        };
        (Arc::new(gate), receiver)
    }

    fn cleanup_trust_directory(directory: PathBuf) {
        let _ = fs::remove_dir_all(directory);
    }
}
