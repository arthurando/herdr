# Local fork: terminal session panel

The lower aggregate panel includes terminal panes even when the server cannot
identify their agent. Existing detected agents retain their names and status.
Undetected panes use a neutral terminal label and a hollow status bubble in the
theme's neutral color. Detection failure does not remove a session from navigation.

Rows use machine plus pane identity. Equal pane IDs on different machines remain
separate. A machine switch preserves the combined list. Cached rows from a lost
connection remain dim; keyboard targets include online machines only. Explicit
agent views still apply their filter, including legacy server-provided order.

The live Sid VM API returned two panes and an empty agents list on 2026-10-28.
This explains why the previous agent-only panel omitted Livestream and SinSang.
This change does not claim that remote agent detection itself is repaired.

Implementation: src/client/shell/aggregate_navigation.rs provides shared rows;
endpoint_agents.rs renders them through agent_sidebar.rs. No wire change or
remote server restart is required.

By default, each row shows the status icon and stripped terminal title on the first line. If
absent, it shows the pane title, then the workspace name. The second line shows
model metadata. If no model is reported, the detected agent name is
shown. An undetected pane shows `terminal`; its agent identity is unknown.
Model metadata is optional and must be supplied by the agent integration.
Existing custom sidebar layouts remain supported.
