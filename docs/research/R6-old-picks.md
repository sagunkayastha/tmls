# R6 — picks carried over from the tmls feature survey (2026-09-30)

Source: picker page https://claude.ai/artifact/BYzRPajmDwjFWppwCsqkZq, collection `picks` (151 features).
Notes starting "Claude's call" were Claude's picks where the user left it blank. Read 2026-10-07.

Counts: want 31, maybe 32, skip 88.

## Want (go into the tmls-rs parity/feature list)

| Feature | Note |
|---|---|
| `agent-orchestration-api` | Must have! Claude has it's own system , use that. for codex and other check global Claude.md |
| `agent-presets` |  |
| `auto-name` |  |
| `clickable-links` |  |
| `context-meter` |  |
| `conversation-title` |  |
| `copy-options` |  |
| `create-session` |  |
| `desktop-notify` | But there should be an option to easily turn off the notification |
| `font-zoom` |  |
| `image-paste-drop` |  |
| `keyboard-sidebar` | Claude's call: known gap, small |
| `kill-session` |  |
| `needs-input-detection` | Claude's call: gives the ? state you want |
| `notification-inbox` |  |
| `notify-sound` | Same thing, easy option to turn on/off |
| `notify-suppress-focused` |  |
| `prompt-history` |  |
| `queue-message` |  |
| `quick-select` |  |
| `remote-approve` |  |
| `rename-session` |  |
| `saved-prompts` | Command: What's the progress |
| `scroll-preserve` |  |
| `session-colors-icons` |  |
| `smart-ctrl-c-paste` |  |
| `status-states-richer` |  |
| `tab-status-badge` |  |
| `tmux-wrap-local` |  |
| `url-screenshot` |  |
| `window-pane-actions` |  |

## Maybe (decide per phase)

| Feature | Note |
|---|---|
| `archive-sessions` |  |
| `board-per-session-context` |  |
| `chat-channel-control` |  |
| `clear-stuck-status` | Just one simple refresh buttopn for whole program |
| `configured-hosts-only` |  |
| `create-with-prompt` | Claude's call: combines your create/preset/saved-prompt picks |
| `directory-picker` | Let user pick directory |
| `element-capture` |  |
| `live-board-sync` |  |
| `mobile-extra-keys` |  |
| `multiple-boards` |  |
| `notify-click-focus` | If it opens inside our ui yes, if it opens in a new terminal no |
| `process-monitor` |  |
| `progress-indicator` |  |
| `push-to-phone` | I want to explore mobile app option. But once this stage of features are done |
| `pwa` | Claude's call: for the web stage |
| `quick-replies` | Claude's call: goes with remote approve |
| `restart-exited` |  |
| `review-comments-to-agent` |  |
| `scheduled-runs` |  |
| `session-history` |  |
| `sidebar-resize-collapse` |  |
| `split-view` |  |
| `tab-management` |  |
| `task-queue` |  |
| `terminal-to-board` |  |
| `tile-wall` |  |
| `tmls-cli` | Claude's call: overlaps your orchestration API |
| `token-auth` | It will use tailscale personal. Check how we do it in archbox and budger dockers |
| `touch-scroll` |  |
| `web-home-list` |  |
| `web-terminal` |  |

## Skipped (not carried over)

`allowed-origins`, `auto-reconnect`, `board-share-link`, `broadcast-input`, `client-sizing`, `clients-view-kick`, `command-palette`, `composer-box`, `configurable-keys`, `connection-state`, `copy-mode-keys`, `device-pairing`, `diff-panel`, `doctor`, `error-classification`, `export-scrollback`, `freeze-session`, `fuzzy-switcher`, `global-hotkey`, `hang-detection`, `hibernate-idle`, `hide-push-content`, `hide-sessions`, `host-daemon`, `hot-reload-config`, `hover-details`, `idle-timeout`, `jump-next-attention`, `keepalive-grace`, `last-message-preview`, `leave-page-alert`, `mobile-fit-pan`, `mobile-layout`, `mobile-viewport`, `mosh-transport`, `multi-select`, `next-prev-session`, `notify-cli`, `notify-command-hook`, `notify-context`, `notify-settings`, `number-jump`, `numbered-markup`, `osc-bell-notify`, `paste-safety`, `phone-photo-attach`, `pin-sessions`, `port-forwarding`, `predictive-echo`, `prompt-jump`, `proxy-auth`, `read-only-view`, `reattach-existing`, `recent-sessions`, `remote-agent-status`, `reorder-sessions`, `reset-terminal`, `restore-tabs`, `resurrect-sessions`, `reverse-proxy-path`, `row-density`, `row-metadata`, `scrollback-search`, `session-aliases`, `session-end-reason`, `session-groups`, `session-notes`, `session-preview`, `session-templates`, `session-url`, `sidebar-filter`, `ssh-config-hosts`, `start-from-phone`, `status-board`, `status-host-filters`, `status-via-hooks`, `subagent-rows`, `synced-drafts`, `terminal-images`, `terminal-recording`, `transcript-search`, `transcript-view`, `unread-count-badge`, `unread-marks`, `usage-limits`, `wake-host`, `window-pane-tree`, `worktree-sessions`
