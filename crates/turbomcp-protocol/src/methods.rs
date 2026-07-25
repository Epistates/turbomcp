//! Wire method and notification names, as string constants.
//!
//! Centralizing these keeps the `MethodRouter` and per-version handlers from
//! re-typing string literals (and silently disagreeing). Names that exist in
//! only one version are noted.

/// Request method names.
pub mod request {
    /// `server/discover` — stateless capability discovery (`2026-07-28`).
    pub const DISCOVER: &str = "server/discover";
    /// `initialize` — stateful handshake (`2025-11-25` and earlier).
    pub const INITIALIZE: &str = "initialize";
    /// `ping` — liveness probe (core in `2025-11-25`).
    pub const PING: &str = "ping";
    /// `tools/list` — enumerate tools.
    pub const TOOLS_LIST: &str = "tools/list";
    /// `tools/call` — invoke a tool.
    pub const TOOLS_CALL: &str = "tools/call";
    /// `resources/list` — enumerate resources.
    pub const RESOURCES_LIST: &str = "resources/list";
    /// `resources/templates/list` — enumerate resource templates.
    pub const RESOURCES_TEMPLATES_LIST: &str = "resources/templates/list";
    /// `resources/read` — read a resource.
    pub const RESOURCES_READ: &str = "resources/read";
    /// `prompts/list` — enumerate prompts.
    pub const PROMPTS_LIST: &str = "prompts/list";
    /// `prompts/get` — render a prompt.
    pub const PROMPTS_GET: &str = "prompts/get";
    /// `completion/complete` — argument autocompletion.
    pub const COMPLETION_COMPLETE: &str = "completion/complete";
    /// `tasks/list` — enumerate tasks (core in `2025-11-25`; extension in draft).
    pub const TASKS_LIST: &str = "tasks/list";
    /// `tasks/get` — poll a task's status (core in `2025-11-25`).
    pub const TASKS_GET: &str = "tasks/get";
    /// `tasks/cancel` — request cancellation of a task (core in `2025-11-25`).
    pub const TASKS_CANCEL: &str = "tasks/cancel";
    /// `tasks/update` — answer a task's outstanding `inputRequests`
    /// (SEP-2663; the draft extension only — `2025-11-25` core Tasks has no
    /// in-execution input).
    pub const TASKS_UPDATE: &str = "tasks/update";
    /// `tasks/result` — retrieve a task's final result, blocking until the task
    /// reaches a terminal status (core in `2025-11-25`).
    pub const TASKS_RESULT: &str = "tasks/result";
    /// `subscriptions/listen` — open a long-lived notification stream
    /// (`2026-07-28`; replaces `resources/subscribe` and the HTTP GET
    /// stream). The request gets no JSON-RPC response — the stream's first
    /// message is `notifications/subscriptions/acknowledged`.
    pub const SUBSCRIPTIONS_LISTEN: &str = "subscriptions/listen";
    /// `resources/subscribe` — subscribe to one resource's updates
    /// (`2025-11-25`; the draft uses `subscriptions/listen` instead).
    pub const RESOURCES_SUBSCRIBE: &str = "resources/subscribe";
    /// `resources/unsubscribe` — drop a `resources/subscribe` subscription
    /// (`2025-11-25`).
    pub const RESOURCES_UNSUBSCRIBE: &str = "resources/unsubscribe";
    /// `logging/setLevel` — per-session minimum log severity (`2025-11-25`;
    /// the draft replaced it with the per-request `_meta` `logLevel` key).
    pub const LOGGING_SET_LEVEL: &str = "logging/setLevel";

    // ---- server → client ----------------------------------------------------
    //
    // These travel the other way: the *server* issues them and the *client*
    // answers. They ride MRTR `inputRequests` on the draft and inline bidi
    // requests on `2025-11-25`, so both the server's request builder and the
    // client's dispatcher must name them identically — which is why they live
    // here rather than as literals at each end.

    /// `elicitation/create` — ask the client's user for input (form or URL
    /// mode). Completion of a URL-mode interaction is reported by
    /// [`notification::ELICITATION_COMPLETE`](super::notification::ELICITATION_COMPLETE).
    pub const ELICITATION_CREATE: &str = "elicitation/create";
    /// `sampling/createMessage` — ask the client to run an LLM completion.
    /// Deprecated upstream but functional on both versions.
    pub const SAMPLING_CREATE_MESSAGE: &str = "sampling/createMessage";
    /// `roots/list` — ask the client which filesystem roots the server may
    /// operate on. Deprecated upstream but functional on both versions.
    pub const ROOTS_LIST: &str = "roots/list";
}

/// Notification method names (no response).
pub mod notification {
    /// `notifications/initialized` — client finished initializing (stateful).
    pub const INITIALIZED: &str = "notifications/initialized";
    /// `notifications/cancelled` — a previously issued request is cancelled.
    pub const CANCELLED: &str = "notifications/cancelled";
    /// `notifications/tasks/status` — a task's status changed (optional per
    /// spec; requestors must poll `tasks/get` regardless).
    pub const TASKS_STATUS: &str = "notifications/tasks/status";
    /// `notifications/subscriptions/acknowledged` — first message on a
    /// `subscriptions/listen` stream: the filter subset the server honors.
    pub const SUBSCRIPTIONS_ACKNOWLEDGED: &str = "notifications/subscriptions/acknowledged";
    /// `notifications/tools/list_changed` — the tool list changed.
    pub const TOOLS_LIST_CHANGED: &str = "notifications/tools/list_changed";
    /// `notifications/resources/list_changed` — the resource list changed.
    pub const RESOURCES_LIST_CHANGED: &str = "notifications/resources/list_changed";
    /// `notifications/resources/updated` — a subscribed resource changed.
    pub const RESOURCES_UPDATED: &str = "notifications/resources/updated";
    /// `notifications/prompts/list_changed` — the prompt list changed.
    pub const PROMPTS_LIST_CHANGED: &str = "notifications/prompts/list_changed";
    /// `notifications/progress` — progress for a request that carried a
    /// `progressToken`, delivered on that request's own stream.
    pub const PROGRESS: &str = "notifications/progress";
    /// `notifications/message` — a structured log message (`logging`
    /// capability); request-scoped on the draft, session-scoped on legacy.
    pub const MESSAGE: &str = "notifications/message";
    /// `notifications/elicitation/complete` — an out-of-band interaction
    /// started by a URL-mode `elicitation/create` finished. Optional (spec
    /// MAY); goes only to the client that initiated it and names that
    /// request's `elicitationId`.
    pub const ELICITATION_COMPLETE: &str = "notifications/elicitation/complete";
}
